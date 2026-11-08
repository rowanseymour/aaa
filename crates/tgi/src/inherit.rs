//! Template inheritance: a block's `Template=` or `Inherit=` attribute names a
//! `[Template <Name>]` block whose contents it builds on, and that template
//! may name a parent of its own.
//!
//! A child's field replaces the inherited field with the same key. A child's
//! block merges into the inherited block of the same kind, recursively,
//! except for kinds the schema files declare as lists (`[list Property]`),
//! whose blocks are added after the inherited ones. Blocks nested in the
//! result keep their own `Template=`, to be resolved in turn if needed,
//! except where a child's block names a different template from the block it
//! merges into: then its template's fields override the inherited block's,
//! so both are resolved first.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::{Block, Item};

type ByName<'a> = HashMap<String, &'a Block>;

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

    /// The templates each of a sequence of files, in the order they load,
    /// sees as the game reads them: its own, and those of the files before
    /// it that hold nothing but templates; a file that holds anything else
    /// lets its templates go once read. `None` for a file that defines a
    /// template already seen, which fails to load whole.
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
            let only_templates = (root.blocks())
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
        (self.by_name.get(&name))
            .or_else(|| self.shared.get(&name))
            .copied()
    }

    /// The template a block inherits from, if it names one that exists.
    pub fn parent(&self, block: &Block) -> Option<&'a Block> {
        self.get(template(block)?)
    }

    /// The block with everything it inherits merged in. The chain stops at a
    /// missing template or one already visited.
    pub fn resolve(&self, block: &Block) -> Block {
        self.resolve_in(block, &mut Vec::new())
    }

    /// `active` holds the template pairs being merged by re-templated nested
    /// blocks (see [`Templates::merge`]), outermost first.
    fn resolve_in(&self, block: &Block, active: &mut Vec<(String, String)>) -> Block {
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
        };
        for b in chain.iter().rev() {
            self.merge(&mut out.items, &b.items, false, active);
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
    fn merge(
        &self,
        into: &mut Vec<Item>,
        items: &[Item],
        shared: bool,
        active: &mut Vec<(String, String)>,
    ) {
        // Only inherited items are overridden, so a block may repeat a key.
        let inherited = into.len();
        for item in items {
            let existing = into[..inherited]
                .iter()
                .position(|e| self.overrides(item, e));
            if existing.is_none() && shared && matches!(item, Item::Block(_)) && into.contains(item)
            {
                continue;
            }
            match (existing.map(|i| &mut into[i]), item) {
                (Some(Item::Block(e)), Item::Block(b))
                    if self.retemplates(e, b) && !active.contains(&pair(e, b)) =>
                {
                    active.push(pair(e, b));
                    let mut merged = self.resolve_in(e, active);
                    let child = self.resolve_in(b, active);
                    self.merge(&mut merged.items, &child.items, true, active);
                    active.pop();
                    // The inherited block's other attributes stay, under the
                    // child's.
                    merged.header.words = b.header.words.clone();
                    merged.header.attrs = e.header.attrs.clone();
                    set_attrs(&mut merged.header.attrs, &b.header.attrs);
                    merged.header.attrs.retain(|(k, _)| !is_template_attr(k));
                    merged.braced = b.braced;
                    merged.line = b.line;
                    *e = merged;
                }
                (Some(Item::Block(e)), Item::Block(b)) => {
                    e.header.words = b.header.words.clone();
                    set_attrs(&mut e.header.attrs, &b.header.attrs);
                    e.braced = b.braced;
                    e.line = b.line;
                    self.merge(&mut e.items, &b.items, shared, active);
                }
                (Some(e), _) => *e = item.clone(),
                (None, _) => into.push(item.clone()),
            }
        }
    }

    /// Whether `child` names a template other than the one `inherited`
    /// does.
    fn retemplates(&self, inherited: &Block, child: &Block) -> bool {
        match (template(inherited), template(child)) {
            (_, None) => false,
            (Some(a), Some(b)) => !a.eq_ignore_ascii_case(b),
            (None, Some(_)) => true,
        }
    }

    /// Whether `item` takes the place of the inherited `existing`.
    fn overrides(&self, item: &Item, existing: &Item) -> bool {
        match (item, existing) {
            (Item::Field(f), Item::Field(e)) => f.key.eq_ignore_ascii_case(&e.key),
            (Item::Block(b), Item::Block(e)) => {
                let kind = b.kind().unwrap_or("").to_ascii_lowercase();
                !self.lists.contains(&kind)
                    && slot(b).eq_ignore_ascii_case(slot(e))
                    && e.kind().unwrap_or("").eq_ignore_ascii_case(&kind)
            }
            _ => false,
        }
    }
}

/// A document's `[Template]` blocks, by lower-case name.
fn templates_of(root: &Block) -> impl Iterator<Item = (String, &Block)> {
    (root.blocks_of("Template")).filter_map(|b| Some((b.name()?.to_ascii_lowercase(), b)))
}

/// Adds the lower-case kinds `block` declares as lists, at any depth.
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

fn template(block: &Block) -> Option<&str> {
    block.attr("Template").or_else(|| block.attr("Inherit"))
}

/// What a schema's slot declaration (`[list Property]`, `[optional OnDeath]`)
/// declares; every one has the same kind, so the kind alone can't pair them.
fn slot(block: &Block) -> &str {
    match block.kind() {
        Some(k) if k.eq_ignore_ascii_case("list") || k.eq_ignore_ascii_case("optional") => {
            block.name().unwrap_or("")
        }
        _ => "",
    }
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
