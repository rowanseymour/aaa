//! Which block kinds the definitions read, and as what: the one list both
//! [`GameData::coverage`](crate::GameData::coverage) counts from and
//! [`decode`] checks each read against, so the count can't fall behind what
//! is read.

use std::any::TypeId;
use std::collections::HashMap;
use std::sync::LazyLock;

use serde::de::DeserializeOwned;

use crate::defs;

/// Counts what reading a block as one type reads of it.
type Counter = fn(&mut tgi::Coverage, &tgi::Block, Option<&tgi::Templates>);

/// A type a block kind is read as.
struct Read {
    ty: TypeId,
    count: Counter,
}

fn read<T: DeserializeOwned + 'static>() -> Read {
    Read {
        ty: TypeId::of::<T>(),
        count: |c, block, templates| drop(c.read::<T>(block, templates)),
    }
}

/// Each kind, lower-case, with the types it's read as.
static READS: LazyLock<HashMap<&str, Vec<Read>>> = LazyLock::new(|| {
    HashMap::from([
        ("abilitydefinition", vec![read::<defs::AbilityDefinition>()]),
        ("actorcommand", vec![read::<defs::ActorCommand>()]),
        ("actorstate", vec![read::<defs::ActorState>()]),
        ("audio", vec![read::<defs::Audio>()]),
        ("banner", vec![read::<defs::Banner>()]),
        ("biome", vec![read::<defs::Biome>()]),
        ("bone", vec![read::<defs::Bone>()]),
        ("campaign", vec![read::<defs::Campaign>()]),
        ("cursor", vec![read::<defs::Cursor>()]),
        ("damagetype", vec![read::<defs::DamageType>()]),
        ("description", vec![read::<defs::Description>()]),
        ("emblem", vec![read::<defs::Emblem>()]),
        ("faction", vec![read::<defs::Faction>()]),
        (
            "formation",
            vec![
                read::<defs::Formation>(),
                read::<defs::FormationProperties>(),
            ],
        ),
        ("glyph", vec![read::<defs::Glyph>()]),
        ("groupformation", vec![read::<defs::GroupFormation>()]),
        ("handicap", vec![read::<defs::Handicap>()]),
        (
            "hotkeyconfigurationdata",
            vec![read::<defs::HotkeyConfiguration>()],
        ),
        ("kingdomcolor", vec![read::<defs::KingdomColor>()]),
        ("mainmenuinterface", vec![read::<defs::Interface>()]),
        ("minimap", vec![read::<defs::Minimap>()]),
        ("moderninterface", vec![read::<defs::ModernInterface>()]),
        ("nation", vec![read::<defs::Nation>()]),
        (
            "organizationlayout",
            vec![read::<defs::OrganizationLayout>()],
        ),
        ("property", vec![read::<defs::Property>()]),
        ("resource", vec![read::<defs::Resource>()]),
        ("sai", vec![read::<defs::Sai>()]),
        ("sun", vec![read::<defs::Sun>()]),
        ("svars", vec![read::<defs::SVars>()]),
        ("targettype", vec![read::<defs::TargetType>()]),
        ("terrain", vec![read::<defs::Terrain>()]),
        ("thing", vec![read::<defs::Thing>(), read::<defs::Vfx>()]),
        ("unit", vec![read::<defs::Thing>(), read::<defs::Vfx>()]),
        (
            "projectile",
            vec![read::<defs::Thing>(), read::<defs::Vfx>()],
        ),
        ("thingcategory", vec![read::<defs::ThingCategory>()]),
        ("transitionset", vec![read::<defs::TransitionSet>()]),
        ("translate", vec![read::<defs::Translate>()]),
        ("vars", vec![read::<defs::AVars>(), read::<defs::UVars>()]),
        ("vfx", vec![read::<defs::Vfx>()]),
        ("worldrule", vec![read::<defs::WorldRule>()]),
        // The choices of a world rule, in blocks of its own kind.
        ("difficultylevel", vec![read::<defs::WorldRuleChoice>()]),
        // A string table, every line of which is read.
        ("text", vec![read::<HashMap<String, String>>()]),
    ])
});

/// The kinds, lower-case, with fields read one by one rather than as a
/// type, which coverage can't see.
pub const BY_HAND: [&str; 4] = ["connectionstatusdialog", "gameevent", "messagetype", "vars"];

/// A block read as `T`, resolved against `templates` if given. `T` must be
/// one of the types [`READS`] lists for the block's kind.
pub(crate) fn decode<T: DeserializeOwned + 'static>(
    block: &tgi::Block,
    templates: Option<&tgi::Templates>,
) -> Result<T, tgi::Error> {
    debug_assert!(
        reads(block).iter().any(|r| r.ty == TypeId::of::<T>()),
        "{} blocks are read as {}, which tgdata's READS doesn't list",
        block.kind().unwrap_or(""),
        std::any::type_name::<T>(),
    );
    match templates {
        Some(t) => t.deserialize(block),
        None => tgi::from_block(block),
    }
}

fn reads(block: &tgi::Block) -> &'static [Read] {
    let kind = block.kind().unwrap_or("").to_ascii_lowercase();
    READS.get(kind.as_str()).map_or(&[], Vec::as_slice)
}

/// Counts into `coverage` what the definitions read of `block`, or every
/// key unread if they don't read its kind.
pub(crate) fn count(
    coverage: &mut tgi::Coverage,
    block: &tgi::Block,
    templates: Option<&tgi::Templates>,
) {
    let reads = reads(block);
    if reads.is_empty() {
        coverage.skip(block, templates);
        return;
    }
    let readings = reads.iter().map(|r| {
        let mut c = tgi::Coverage::default();
        // Blocks that don't deserialize still count what they read.
        (r.count)(&mut c, block, templates);
        c
    });
    coverage.add(tgi::Coverage::union(readings));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counted(text: &str) -> Vec<(String, String, tgi::Count)> {
        let parsed = tgi::parse(text);
        let mut coverage = tgi::Coverage::default();
        for block in parsed.root.blocks() {
            count(&mut coverage, block, None);
        }
        (coverage.keys())
            .map(|(p, k, c)| (p.to_string(), k.to_string(), c))
            .collect()
    }

    #[test]
    fn kinds_nothing_reads_count_as_unread() {
        let counts = counted("[Ego]\n{\nIDS = low_income\n}\n");
        assert_eq!(counts[0].2, tgi::Count { read: 0, unread: 1 });
    }

    #[test]
    fn a_kind_read_as_several_types_counts_each_key_once() {
        // Every thing is read as a template and tried as an effect.
        let counts = counted("[Thing]\n{\nIDS = tank\ncategory = VEHICLE\n}\n");
        let read = |key: &str| counts.iter().find(|(_, k, _)| k == key).unwrap().2;
        assert_eq!(read("ids"), tgi::Count { read: 1, unread: 0 });
        assert_eq!(read("category"), tgi::Count { read: 1, unread: 0 });
    }
}
