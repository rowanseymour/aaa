//! Template inheritance: a block's `Template=` or `Inherit=` attribute names a
//! `[Template <Name>]` block whose contents it builds on, and that template
//! may name a parent of its own.
//!
//! A child's field replaces the inherited field with the same key. A child's
//! block merges into the inherited block of the same kind, recursively,
//! except for kinds the schema files declare as lists (`[list Property]`),
//! whose blocks are added after the inherited ones. Blocks nested in the
//! result keep their own `Template=`, to be resolved in turn if needed.

use std::collections::{HashMap, HashSet};

use crate::{Block, Item};

/// The `[Template]` blocks of a set of files, by name.
pub struct Templates<'a> {
    /// By lower-case name; the first of duplicate names wins.
    by_name: HashMap<String, &'a Block>,
    /// Lower-case kinds declared as lists.
    lists: HashSet<String>,
}

impl<'a> Templates<'a> {
    /// Indexes the templates and list declarations in the given documents.
    pub fn new(roots: impl IntoIterator<Item = &'a Block>) -> Self {
        let mut t = Templates {
            by_name: HashMap::new(),
            lists: HashSet::new(),
        };
        for root in roots {
            for b in root.blocks_of("Template") {
                if let Some(name) = b.name() {
                    t.by_name.entry(name.to_ascii_lowercase()).or_insert(b);
                }
            }
            t.find_lists(root);
        }
        t
    }

    fn find_lists(&mut self, block: &Block) {
        let words = &block.header.words;
        if let Some(i) = words.iter().position(|w| w.eq_ignore_ascii_case("list"))
            && let Some(kind) = words.get(i + 1)
        {
            self.lists.insert(kind.to_ascii_lowercase());
        }
        for b in block.blocks() {
            self.find_lists(b);
        }
    }

    /// A template by name, case-insensitive.
    pub fn get(&self, name: &str) -> Option<&'a Block> {
        self.by_name.get(&name.to_ascii_lowercase()).copied()
    }

    /// The template a block inherits from, if it names one that exists.
    pub fn parent(&self, block: &Block) -> Option<&'a Block> {
        self.get(block.attr("Template").or_else(|| block.attr("Inherit"))?)
    }

    /// The block with everything it inherits merged in. The chain stops at a
    /// missing template or one already visited.
    pub fn resolve(&self, block: &Block) -> Block {
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
            self.merge(&mut out.items, &b.items);
        }
        out
    }

    fn merge(&self, into: &mut Vec<Item>, items: &[Item]) {
        // Only inherited items are overridden, so a block may repeat a key.
        let inherited = into.len();
        for item in items {
            let existing = into[..inherited]
                .iter()
                .position(|e| self.overrides(item, e));
            match (existing.map(|i| &mut into[i]), item) {
                (Some(Item::Block(e)), Item::Block(b)) => {
                    e.header.words = b.header.words.clone();
                    for (k, v) in &b.header.attrs {
                        let attrs = &mut e.header.attrs;
                        match attrs.iter_mut().find(|(ek, _)| ek.eq_ignore_ascii_case(k)) {
                            Some(attr) => *attr = (k.clone(), v.clone()),
                            None => attrs.push((k.clone(), v.clone())),
                        }
                    }
                    e.braced = b.braced;
                    e.line = b.line;
                    self.merge(&mut e.items, &b.items);
                }
                (Some(e), _) => *e = item.clone(),
                (None, _) => into.push(item.clone()),
            }
        }
    }

    /// Whether `item` takes the place of the inherited `existing`.
    fn overrides(&self, item: &Item, existing: &Item) -> bool {
        match (item, existing) {
            (Item::Field(f), Item::Field(e)) => f.key.eq_ignore_ascii_case(&e.key),
            (Item::Block(b), Item::Block(e)) => {
                let kind = b.kind().unwrap_or("").to_ascii_lowercase();
                !self.lists.contains(&kind) && e.kind().unwrap_or("").eq_ignore_ascii_case(&kind)
            }
            _ => false,
        }
    }
}
