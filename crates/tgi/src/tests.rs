use super::*;

fn ok(text: &str) -> Block {
    let p = parse(text);
    assert_eq!(p.warnings, [], "unexpected warnings");
    p.root
}

fn warnings(text: &str) -> Vec<String> {
    parse(text)
        .warnings
        .into_iter()
        .map(|w| format!("{}: {}", w.line, w.message))
        .collect()
}

#[test]
fn braced_and_ini_style_blocks() {
    let root = ok("
[Thing Template=BaseTank]
{
    IDS = test_tank
    name = \"#test_tank_name\"
    [Body]
    health = 200
    defense = 1
    [Mover]
    speed = 3
    [Abilities]
    {
        [Ability Template=Gun]
        range = 10
    }
}
");
    let thing = root.blocks().next().unwrap();
    assert_eq!(
        (thing.kind(), thing.attr("template"), thing.braced),
        (Some("Thing"), Some("BaseTank"), true)
    );
    assert_eq!(thing.value("ids"), Some("test_tank"));
    assert!(thing.field("name").unwrap().quoted);

    let kinds: Vec<_> = thing.blocks().map(|b| b.kind().unwrap()).collect();
    assert_eq!(kinds, ["Body", "Mover", "Abilities"]);
    let body = thing.blocks_of("body").next().unwrap();
    assert_eq!(
        (body.value("health"), body.value("defense"), body.braced),
        (Some("200"), Some("1"), false)
    );
    assert_eq!(body.line, 6);
    let gun = thing
        .blocks_of("Abilities")
        .next()
        .unwrap()
        .blocks()
        .next()
        .unwrap();
    assert_eq!(gun.value("range"), Some("10"));
}

#[test]
fn ini_sections_at_top_level() {
    let root = ok("[A]\nx = 1\n[B]\ny = 2\n");
    let names: Vec<_> = root
        .blocks()
        .map(|b| (b.kind().unwrap(), b.fields().count()))
        .collect();
    assert_eq!(names, [("A", 1), ("B", 1)]);
}

#[test]
fn schema_fields() {
    let root = ok(
        "[template Body]\n{\n\tfixed\thealth\n\tflag destructible = true\n\tstring icon = NULL\n\t[optional OnDeath template=Ability]\n}",
    );
    let t = root.blocks().next().unwrap();
    assert_eq!((t.kind(), t.name()), (Some("template"), Some("Body")));
    let f: Vec<_> = t
        .fields()
        .map(|f| (f.ty.as_deref(), f.key.as_str(), f.value.as_deref()))
        .collect();
    assert_eq!(
        f,
        [
            (Some("fixed"), "health", None),
            (Some("flag"), "destructible", Some("true")),
            (Some("string"), "icon", Some("NULL"))
        ]
    );
    let opt = t.blocks().next().unwrap();
    assert_eq!(
        (opt.header.words.as_slice(), opt.attr("Template")),
        (
            &["optional".to_string(), "OnDeath".into()][..],
            Some("Ability")
        )
    );
}

#[test]
fn comments() {
    let root = ok(
        "/* header\n   comment */[A] ; trailing\n;; whole line\nurl = http://example.com/x ; note\ntip = \"a;b\" ;\"old text\"\n[B]/* x */ ",
    );
    assert_eq!(root.blocks().count(), 2);
    let a = root.blocks().next().unwrap();
    assert_eq!(a.line, 2);
    assert_eq!(a.value("url"), Some("http://example.com/x"));
    assert_eq!(a.value("tip"), Some("a;b"));
}

#[test]
fn header_attribute_spacing() {
    let h = |s: &str| ok(s).blocks().next().unwrap().header.clone();
    let attrs = |s: &str| h(s).attrs;
    assert_eq!(
        attrs("[Set template = Chars]"),
        [("template".into(), "Chars".into())]
    );
    assert_eq!(
        attrs("[Set template= Chars]"),
        [("template".into(), "Chars".into())]
    );
    assert_eq!(
        attrs("[Set template =Chars]"),
        [("template".into(), "Chars".into())]
    );
    let h2 = h("[Template X Template=Y Inherit=Z]");
    assert_eq!(h2.words, ["Template", "X"]);
    assert_eq!(
        h2.attrs,
        [
            ("Template".into(), "Y".into()),
            ("Inherit".into(), "Z".into())
        ]
    );
}

#[test]
fn block_after_closing_brace_on_same_line() {
    let root = ok("[A]\n{\n}[B]\n{\n}");
    assert_eq!(root.blocks().count(), 2);
}

#[test]
fn empty_braces_and_key_with_bracket() {
    let root = ok("[A]\n{}\n[Strings]\nkey_[ = \"[\"");
    assert_eq!(root.blocks().nth(1).unwrap().value("key_["), Some("["));
}

#[test]
fn lenient_with_warnings() {
    assert_eq!(
        warnings("[Thing template=X\n{\n}"),
        ["1: header missing ']'"]
    );
    assert_eq!(warnings("[D = Template=D]"), ["1: stray '=' in header"]);
    assert_eq!(
        warnings("[Font] notes about the font"),
        ["1: ignoring text after header: \"notes about the font\""]
    );
    assert_eq!(warnings("}"), ["1: unmatched '}'"]);
    assert_eq!(
        warnings("[A]\n{\nx = 1"),
        ["1: block not closed before end of file"]
    );
    assert_eq!(warnings("/* never ends"), ["1: unterminated /* comment"]);

    assert_eq!(warnings("{\n}"), ["1: '{' without a preceding header"]);

    let root = parse("[Thing template=X\n{\n a = 1\n}").root;
    let t = root.blocks().next().unwrap();
    assert_eq!((t.attr("template"), t.value("a")), (Some("X"), Some("1")));
}

#[test]
fn brace_after_section_fields_continues_section() {
    let p = parse("[Thing]\n{\n\t[Size]\n\tscale = 1\n\t{\n\t\tradius = 11\n\t}\n\t[Next]\n}");
    assert_eq!(
        p.warnings,
        [Warning {
            line: 5,
            message: "'{' after the section's fields; continuing the section".into()
        }]
    );
    let thing = p.root.blocks().next().unwrap();
    let size = thing.blocks_of("Size").next().unwrap();
    assert_eq!(
        (size.value("scale"), size.value("radius")),
        (Some("1"), Some("11"))
    );
    assert_eq!(thing.blocks().count(), 2);
}

#[test]
fn unbalanced_quote_kept_literally() {
    let root = ok("[A]\ntexture = a/b.tga\"");
    assert_eq!(
        root.blocks().next().unwrap().value("texture"),
        Some("a/b.tga\"")
    );
}

#[test]
fn inheritance() {
    let schema = ok("
[template Thing]
{
    [list Property]
    [list TextureSearchPath]
}
");
    let files = ok("
[Template Base Template=Thing]
{
    health = 100
    [View]
    {
        [TextureSearchPath]
        path = /Shared
        [AspectNormal Template=Aspect]
        {
            [Animation]
            ids = idle
        }
    }
    [PropertiesComponent]
    {
        [Property]
        property_ids = a
    }
}
[template Middle inherit=base]
{
    health = 200
    [Size]
    scale_min = 2
    [View]
    {
        resource_prefix = Corps/Corps
        [AspectNormal]
        skin = corps.dds
    }
}
[Thing Template=Middle]
{
    IDS = corps
    [PropertiesComponent]
    {
        [Property]
        property_ids = b
    }
    [Size]
    scale_min = 3
}
[Template Loop1 Template=Loop2]
x = 1
[Template Loop2 Template=Loop1]
x = 2
[Thing Template=Loop1]
[Thing Template=Missing]
y = 1
");
    let templates = Templates::new([&schema, &files]);
    let things: Vec<&Block> = files.blocks_of("Thing").collect();
    let corps = templates.resolve(things[0]);
    assert_eq!(corps.header, things[0].header);
    assert_eq!(
        (corps.value("ids"), corps.value("health")),
        (Some("corps"), Some("200"))
    );
    let size: Vec<_> = corps.blocks_of("Size").collect();
    assert_eq!(size.len(), 1);
    assert_eq!(size[0].value("scale_min"), Some("3"));

    let view = corps.blocks_of("View").next().unwrap();
    assert_eq!(view.value("resource_prefix"), Some("Corps/Corps"));
    assert_eq!(view.blocks_of("TextureSearchPath").count(), 1);
    let normal: Vec<_> = view.blocks_of("AspectNormal").collect();
    assert_eq!(normal.len(), 1);
    assert_eq!(
        (normal[0].attr("template"), normal[0].value("skin")),
        (Some("Aspect"), Some("corps.dds"))
    );
    assert_eq!(normal[0].blocks_of("Animation").count(), 1);

    let properties: Vec<_> = corps
        .blocks_of("PropertiesComponent")
        .flat_map(|c| c.blocks_of("Property"))
        .filter_map(|p| p.value("property_ids"))
        .collect();
    assert_eq!(properties, ["a", "b"]);

    let looped = templates.resolve(things[1]);
    assert_eq!(looped.value("x"), Some("1"));
    assert_eq!(&templates.resolve(things[2]), things[2]);
}
