//! Template inheritance: a block's `Template=` or `Inherit=` attribute names a
//! `[Template <Name>]` block whose contents it builds on, and that template
//! may name a parent of its own.
//!
//! A child's field replaces the inherited field with the same key. A child's
//! block merges into the inherited block of the same kind, recursively,
//! except for kinds the schema files declare as lists, whose blocks are
//! added after the inherited ones. Blocks nested in the result keep their
//! own `Template=`, to be resolved in turn if needed, except where a child's
//! block names a different template from the block it merges into: then its
//! template's fields override the inherited block's, so both are resolved
//! first. A child's block naming the same template as the block it merges
//! into, or none, takes only the inherited block's fields: the inherited
//! block already holds what that template gives, under its own fields.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::{Block, Item};

type ByName<'a> = HashMap<String, &'a Block>;

/// Where resolving a block has got to.
#[derive(Default)]
struct Merging {
    /// The template pairs being merged by re-templated nested blocks (see
    /// [`Templates::merge`]), outermost first.
    active: Vec<(String, String)>,
    /// Why the game refuses the block, once that's found.
    refused: Option<String>,
}

/// The `[Template]` blocks of a set of files, by name.
pub struct Templates<'a> {
    /// By lower-case name; the first of duplicate names wins.
    by_name: ByName<'a>,
    /// Templates that other files lend, which `by_name` never shares a name
    /// with.
    shared: Arc<ByName<'a>>,
    /// Lower-case kinds declared as lists.
    lists: Arc<HashSet<String>>,
}

impl<'a> Templates<'a> {
    /// Indexes the templates and list declarations in the given documents.
    pub fn new(roots: impl IntoIterator<Item = &'a Block>) -> Self {
        let mut by_name = HashMap::new();
        let mut lists = HashSet::new();
        for root in roots {
            for (name, b) in templates_of(root) {
                by_name.entry(name).or_insert(b);
            }
            find_lists(root, &mut lists);
        }
        Templates {
            by_name,
            shared: Arc::default(),
            lists: Arc::new(lists),
        }
    }

    /// For each file in load order, the templates it can see, as the game
    /// reads them: its own, plus those of earlier files that hold only
    /// templates. `None` for a file that redefines a template it can see, or
    /// defines one twice, which the game rejects whole.
    pub fn scoped(roots: impl IntoIterator<Item = &'a Block>) -> Vec<Option<Self>> {
        let roots: Vec<&Block> = roots.into_iter().collect();
        let mut lists = HashSet::new();
        for root in &roots {
            find_lists(root, &mut lists);
        }
        let lists = Arc::new(lists);
        let mut shared: Arc<ByName> = Arc::default();
        let mut out = Vec::with_capacity(roots.len());
        for root in roots {
            let mut by_name = HashMap::new();
            let clash = templates_of(root)
                .any(|(name, b)| shared.contains_key(&name) || by_name.insert(name, b).is_some());
            if clash {
                out.push(None);
                continue;
            }
            let only_templates = root
                .blocks()
                .all(|b| b.kind().is_some_and(|k| k.eq_ignore_ascii_case("Template")));
            if only_templates && !by_name.is_empty() {
                let mut grown = (*shared).clone();
                grown.extend(by_name.iter().map(|(k, v)| (k.clone(), *v)));
                shared = Arc::new(grown);
            }
            out.push(Some(Templates {
                by_name,
                shared: shared.clone(),
                lists: lists.clone(),
            }));
        }
        out
    }

    /// A template by name, case-insensitive.
    pub fn get(&self, name: &str) -> Option<&'a Block> {
        let name = name.to_ascii_lowercase();
        self.by_name
            .get(&name)
            .or_else(|| self.shared.get(&name))
            .copied()
    }

    /// The template a block inherits from, if it names one that exists.
    pub fn parent(&self, block: &Block) -> Option<&'a Block> {
        self.get(template(block)?)
    }

    /// The lower-case names of the templates a block inherits from, nearest
    /// first, as far as the chain goes: to a missing template or one already
    /// visited.
    pub fn lineage(&self, block: &Block) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut at = block;
        while let Some(name) = template(at).map(str::to_ascii_lowercase)
            && !out.contains(&name)
        {
            out.push(name);
            match self.parent(at) {
                Some(parent) => at = parent,
                None => break,
            }
        }
        out
    }

    /// The block with everything it inherits merged in. The chain stops at a
    /// missing template or one already visited.
    pub fn resolve(&self, block: &Block) -> Block {
        self.resolve_in(block, &mut Merging::default())
    }

    /// As [`Templates::resolve`], or why the game refuses the block: a nested
    /// block naming a template that neither is nor descends from the one the
    /// block it merges into was built from.
    pub fn try_resolve(&self, block: &Block) -> Result<Block, String> {
        let mut merging = Merging::default();
        let out = self.resolve_in(block, &mut merging);
        merging.refused.map_or(Ok(out), Err)
    }

    fn resolve_in(&self, block: &Block, merging: &mut Merging) -> Block {
        let mut chain = vec![block];
        while let Some(p) = self.parent(chain[chain.len() - 1]) {
            if chain.iter().any(|&b| std::ptr::eq(b, p)) {
                break;
            }
            chain.push(p);
        }
        let mut out = Block {
            header: block.header.clone(),
            items: Vec::new(),
            braced: block.braced,
            line: block.line,
            resolved_from: block.resolved_from.clone(),
        };
        for b in chain.iter().rev() {
            self.merge(&mut out.items, &b.items, false, merging);
        }
        out
    }

    /// Merges `items` over `into`. With `shared`, list blocks already in
    /// `into` aren't added again, as both sides may have them from a
    /// template they have in common.
    ///
    /// A nested block that names another template than the one it merges
    /// into has both resolved first, and resolving them can come back to the
    /// same pair of templates (`[Template A Template=B]` holding
    /// `[View Template=X]` where B holds `[View Template=A]`). A pair already
    /// being merged is merged as plain blocks instead, which ends the cycle;
    /// a template met again under a different pair is fine and common.
    fn merge(&self, into: &mut Vec<Item>, items: &[Item], shared: bool, merging: &mut Merging) {
        // Only inherited items are overridden, so a block may repeat a key.
        let inherited = into.len();
        for item in items {
            let existing = match self.is_list(item) {
                true => None,
                false => into[..inherited].iter().position(|e| overrides(item, e)),
            };
            if existing.is_none() && shared && matches!(item, Item::Block(_)) && into.contains(item)
            {
                continue;
            }
            match (existing.map(|i| &mut into[i]), item) {
                (Some(Item::Block(e)), Item::Block(b))
                    if retemplates(e, b) && !merging.active.contains(&pair(e, b)) =>
                {
                    self.check_line(e, b, merging);
                    merging.active.push(pair(e, b));
                    let mut merged = match self.parent(e) {
                        Some(_) => self.resolve_in(e, merging),
                        None => e.clone(),
                    };
                    let child = self.resolve_in(b, merging);
                    self.merge(&mut merged.items, &child.items, true, merging);
                    merging.active.pop();
                    // The inherited block's other attributes stay, under the
                    // child's.
                    merged.header.words = b.header.words.clone();
                    merged.header.attrs = e.header.attrs.clone();
                    set_attrs(&mut merged.header.attrs, &b.header.attrs);
                    merged.header.attrs.retain(|(k, _)| !is_template_attr(k));
                    merged.braced = b.braced;
                    merged.line = b.line;
                    merged.resolved_from = template(b).map(str::to_string);
                    *e = merged;
                }
                (Some(Item::Block(e)), Item::Block(b)) => {
                    // A pair already being merged, whose child's template is
                    // left to resolve in turn, over what's merged in so far.
                    let cycle = retemplates(e, b);
                    e.header.words = b.header.words.clone();
                    set_attrs(&mut e.header.attrs, &b.header.attrs);
                    if cycle {
                        e.resolved_from = None;
                    } else if e.resolved_from.is_some() {
                        // Its template is merged in already, so mustn't be
                        // again.
                        e.header.attrs.retain(|(k, _)| !is_template_attr(k));
                    }
                    e.braced = b.braced;
                    e.line = b.line;
                    self.merge(&mut e.items, &b.items, shared, merging);
                }
                (Some(e), _) => *e = item.clone(),
                (None, _) => into.push(item.clone()),
            }
        }
    }

    /// Notes in `merging` a `child` naming a template outside the line of the
    /// one `inherited` is built from, where both templates exist.
    fn check_line(&self, inherited: &Block, child: &Block, merging: &mut Merging) {
        let Some(named) = template(inherited).or(inherited.resolved_from.as_deref()) else {
            return;
        };
        let Some(own) = template(child) else {
            return;
        };
        if self.get(named).is_none()
            || self.get(own).is_none()
            || self.lineage(child).contains(&named.to_ascii_lowercase())
        {
            return;
        }
        merging.refused.get_or_insert_with(|| {
            format!(
                "[{}] at line {} names {own}, which isn't {named} or one of its descendants",
                child.kind().unwrap_or(""),
                child.line
            )
        });
    }

    /// Whether `item` is a block of a kind declared as a list, which never
    /// takes the place of an inherited block.
    fn is_list(&self, item: &Item) -> bool {
        match item {
            Item::Block(b) => self
                .lists
                .contains(&b.kind().unwrap_or("").to_ascii_lowercase()),
            Item::Field(_) => false,
        }
    }
}

/// Whether `item` takes the place of the inherited `existing`, unless it's a
/// list block (see [`Templates::is_list`]).
fn overrides(item: &Item, existing: &Item) -> bool {
    match (item, existing) {
        (Item::Field(f), Item::Field(e)) => f.key.eq_ignore_ascii_case(&e.key),
        (Item::Block(b), Item::Block(e)) => {
            // A slot's own kind is just `list` or `optional`, so that
            // alone can't pair them.
            let same_slot = match (b.slot(), e.slot()) {
                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                (a, b) => a.is_none() && b.is_none(),
            };
            same_slot
                && e.kind()
                    .unwrap_or("")
                    .eq_ignore_ascii_case(b.kind().unwrap_or(""))
        }
        _ => false,
    }
}

/// A document's `[Template]` blocks, by lower-case name.
fn templates_of(root: &Block) -> impl Iterator<Item = (String, &Block)> {
    root.blocks_of("Template")
        .filter_map(|b| Some((b.name()?.to_ascii_lowercase(), b)))
}

/// Adds the lower-case kinds `block` declares as lists, at any depth. Not
/// just slots whose kind is `list`: an optional one can be a list too, as
/// in `[optional list Action]`.
fn find_lists(block: &Block, lists: &mut HashSet<String>) {
    let words = &block.header.words;
    if let Some(i) = words.iter().position(|w| w.eq_ignore_ascii_case("list"))
        && let Some(kind) = words.get(i + 1)
    {
        lists.insert(kind.to_ascii_lowercase());
    }
    for b in block.blocks() {
        find_lists(b, lists);
    }
}

/// Whether `child` names a template other than the one `inherited` does or
/// was built from.
fn retemplates(inherited: &Block, child: &Block) -> bool {
    let named = template(inherited).or(inherited.resolved_from.as_deref());
    match (named, template(child)) {
        (_, None) => false,
        (Some(a), Some(b)) => !a.eq_ignore_ascii_case(b),
        (None, Some(_)) => true,
    }
}

fn template(block: &Block) -> Option<&str> {
    block.attr("Template").or_else(|| block.attr("Inherit"))
}

/// The templates an inherited block and the child block over it name.
fn pair(inherited: &Block, child: &Block) -> (String, String) {
    let name = |b| template(b).unwrap_or("").to_ascii_lowercase();
    (name(inherited), name(child))
}

/// Sets each of `over` in `attrs`, replacing one with the same key.
fn set_attrs(attrs: &mut Vec<(String, String)>, over: &[(String, String)]) {
    for (k, v) in over {
        match attrs.iter_mut().find(|(ek, _)| ek.eq_ignore_ascii_case(k)) {
            Some(attr) => *attr = (k.clone(), v.clone()),
            None => attrs.push((k.clone(), v.clone())),
        }
    }
}

fn is_template_attr(key: &str) -> bool {
    key.eq_ignore_ascii_case("Template") || key.eq_ignore_ascii_case("Inherit")
}
