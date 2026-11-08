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
fn typed_values() {
    let root = ok("scale = 1.5\nbad = x\ninvisible = TRUE\nvisible = false\nquoted = \" 2 \"");
    assert_eq!(root.parse_value::<f32>("SCALE"), Some(1.5));
    assert_eq!(root.parse_value::<f32>("bad"), None);
    assert_eq!(root.parse_value::<f32>("missing"), None);
    assert_eq!(root.parse_value::<i32>("quoted"), Some(2));
    assert!(root.flag("invisible"));
    assert!(!root.flag("visible"));
    assert!(!root.flag("missing"));
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

#[test]
fn nested_block_naming_another_template() {
    let schema = ok("[template Definition]\n{\n[list Effect]\n}");
    let files = ok("
[Template Attack Template=Definition]
{
    range = 1
    [Effect]
    kind = base
}
[Template Grenade Inherit=Attack]
{
    range = 9
    recharge_time = 15
    area = 2
}
[Template GrenadeUse]
{
    [Definition Template=Attack]
    {
        recharge_time = 0.5
        area = 1
    }
}
[Ability Template=GrenadeUse]
{
    [Definition Template=Grenade]
    area = 3
}
");
    let templates = Templates::new([&schema, &files]);
    let ability = templates.resolve(files.blocks_of("Ability").next().unwrap());
    let definition = ability.blocks_of("Definition").next().unwrap();
    // The block it merges into, then its own template, then its own fields.
    assert_eq!(definition.value("range"), Some("9"));
    assert_eq!(definition.value("recharge_time"), Some("15"));
    assert_eq!(definition.value("area"), Some("3"));
    assert_eq!(definition.blocks_of("Effect").count(), 1);
    assert_eq!(definition.attr("template"), None);
}

#[test]
fn template_cycle_through_nested_block() {
    let files = ok("
[Template A Template=B]
{
[View Template=X]
}
[Template B]
{
[View Template=A]
}
[Template X]
{
}
[Thing Template=A]
[Template T]
{
[X Template=T]
{
[X Template=U]
}
}
");
    let templates = Templates::new([&files]);
    let thing = templates.resolve(files.blocks_of("Thing").next().unwrap());
    assert_eq!(thing.blocks_of("View").count(), 1);
    let t = templates.get("T").unwrap();
    let nested = t.blocks_of("X").next().unwrap();
    assert_eq!(templates.resolve(nested).blocks_of("X").count(), 1);
}

#[test]
fn slot_declarations_are_kept_apart() {
    let schema = ok("
[template Base]
{
    [list Property]
    [optional OnDeath]
}
[template Child template=Base]
{
    [list Upgrade]
    [optional OnBirth]
    [list Property]
    {
        x = 1
    }
}
");
    let templates = Templates::new([&schema]);
    let child = templates.resolve(templates.get("Child").unwrap());
    let slots: Vec<_> = child
        .blocks()
        .map(|b| format!("{} {}", b.kind().unwrap(), b.name().unwrap()))
        .collect();
    assert_eq!(
        slots,
        [
            "list Property",
            "optional OnDeath",
            "list Upgrade",
            "optional OnBirth"
        ]
    );
    assert_eq!(child.blocks().next().unwrap().value("x"), Some("1"));
}

#[test]
fn block_comments_start_outside_quotes_and_line_comments() {
    let root = ok("[A]\nx = 1 ; was /* off\ny = 2\nz = \"/* kept */\"\n/* gone */ w = 3\n");
    let a = root.blocks().next().unwrap();
    assert_eq!(
        (a.value("x"), a.value("y"), a.value("z"), a.value("w")),
        (Some("1"), Some("2"), Some("/* kept */"), Some("3"))
    );
}
