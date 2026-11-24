use std::collections::BTreeMap;

use galaxy_scene::{Binding, Handle, Host, Kind, ObjectId, Operation, Primitive, Refusal, Session};

fn id(name: &str) -> ObjectId {
    ObjectId::new(name).unwrap()
}

fn add(name: &str, primitive: Primitive, handle: Option<&str>) -> Operation {
    Operation::Add {
        id: id(name),
        primitive,
        handle: handle.map(Handle::new),
    }
}

fn remove(name: &str) -> Operation {
    Operation::Remove { id: id(name) }
}

fn building(on: &str) -> Primitive {
    Primitive::Building { on: id(on) }
}

fn arc(from: &str, to: &str) -> Primitive {
    Primitive::Arc {
        from: id(from),
        to: id(to),
    }
}

fn label(of: &str) -> Primitive {
    Primitive::Label { of: id(of) }
}

fn two_sites() -> Vec<Operation> {
    vec![
        add("earth", Primitive::Globe, None),
        add("a", building("earth"), Some("site:a")),
        add("b", building("earth"), Some("site:b")),
        add("a-b", arc("a", "b"), Some("flow:1")),
        add("a-b.label", label("a-b"), None),
    ]
}

struct Sets(BTreeMap<Handle, Vec<Handle>>);

impl Host for Sets {
    fn members(&self, set: &Handle) -> Option<Vec<Handle>> {
        self.0.get(set).cloned()
    }
}

fn handles(texts: &[&str]) -> Vec<Handle> {
    texts.iter().copied().map(Handle::new).collect()
}

fn host() -> Sets {
    Sets(BTreeMap::from([
        (
            Handle::new("flows:a-b"),
            handles(&["flow:7", "flow:2", "flow:5"]),
        ),
        (Handle::new("flows:b-a"), Vec::new()),
    ]))
}

fn sites_with_flows() -> Session {
    let mut session = session(&two_sites()[..3]);
    session
        .add_each(id("top"), arc("a", "b"), Handle::new("flows:a-b"), &host())
        .unwrap();
    session
}

fn session(log: &[Operation]) -> Session {
    Session::replay(log).unwrap()
}

fn refused(session: &mut Session, operation: Operation) -> Refusal {
    let before = session.clone();
    let refusal = session.apply(operation).unwrap_err();
    assert_eq!(*session, before);
    refusal
}

#[test]
fn object_name_is_1_to_64_ascii_letters_digits_hyphens_underscores_and_dots() {
    for name in ["a", "a-b.label", "Site_01", &"x".repeat(64)] {
        assert_eq!(ObjectId::new(name).unwrap().as_str(), name);
    }
    for name in ["", "a b", "a/b", "é", &"x".repeat(65)] {
        assert_eq!(ObjectId::new(name), None, "{name:?}");
    }
}

#[test]
fn each_primitive_has_one_target_for_each_reference_field_of_its_kind() {
    let primitives = [
        Primitive::Globe,
        building("earth"),
        arc("a", "b"),
        label("a"),
    ];
    let kinds: Vec<Kind> = primitives.iter().map(Primitive::kind).collect();
    assert_eq!(kinds, Kind::ALL);
    for primitive in &primitives {
        assert_eq!(
            primitive.targets().len(),
            primitive.kind().reference_fields().len()
        );
    }
}

#[test]
fn scene_text_has_one_line_for_each_object_in_name_order() {
    let session = session(&two_sites());
    assert_eq!(
        session.scene().describe(),
        "a building on=earth handle=\"site:a\"\n\
         a-b arc from=a to=b handle=\"flow:1\"\n\
         a-b.label label of=a-b\n\
         b building on=earth handle=\"site:b\"\n\
         earth globe\n"
    );
}

#[test]
fn replay_of_the_log_gives_the_same_session() {
    let mut first = session(&two_sites());
    first.apply(remove("a-b.label")).unwrap();
    first.apply(add("b.label", label("b"), None)).unwrap();
    assert_eq!(first.undo(), Some(add("b.label", label("b"), None)));

    let second = session(first.log());
    assert_eq!(second, first);
    assert_eq!(second.scene().describe(), first.scene().describe());
}

#[test]
fn undo_of_an_add_removes_the_object() {
    let mut session = session(&two_sites());
    let before = session.clone();
    session.apply(add("b.label", label("b"), None)).unwrap();
    assert_eq!(session.undo(), Some(add("b.label", label("b"), None)));
    assert_eq!(session, before);
}

#[test]
fn undo_of_a_remove_restores_the_object_with_its_handle() {
    let mut session = session(&two_sites());
    session.apply(remove("a-b.label")).unwrap();
    let before = session.clone();
    session.apply(remove("a-b")).unwrap();
    assert_eq!(session.scene().get(&id("a-b")), None);
    assert_eq!(session.undo(), Some(remove("a-b")));
    assert_eq!(session, before);
    let object = session.scene().get(&id("a-b")).unwrap();
    assert_eq!(object.primitive, arc("a", "b"));
    assert_eq!(object.binding, Binding::One(Handle::new("flow:1")));
}

#[test]
fn undo_of_each_operation_gives_the_empty_session() {
    let mut session = session(&two_sites());
    let mut undone = Vec::new();
    while let Some(operation) = session.undo() {
        undone.push(operation);
    }
    undone.reverse();
    assert_eq!(undone, two_sites());
    assert_eq!(session, Session::new());
    assert!(session.scene().is_empty());
}

#[test]
fn add_with_a_name_in_use_is_refused() {
    let mut session = session(&two_sites());
    let refusal = refused(&mut session, add("a", Primitive::Globe, None));
    assert_eq!(refusal, Refusal::Exists { id: id("a") });
    assert_eq!(refusal.to_string(), "object a exists");
}

#[test]
fn add_with_a_reference_to_no_object_is_refused() {
    let mut session = session(&two_sites());
    let refusal = refused(&mut session, add("c", building("mars"), None));
    assert_eq!(
        refusal,
        Refusal::NoTarget {
            field: "on",
            target: id("mars")
        }
    );
    assert_eq!(refusal.to_string(), "on: no object mars");

    let refusal = refused(&mut session, add("c.label", label("c.label"), None));
    assert_eq!(refusal.to_string(), "of: no object c.label");
}

#[test]
fn add_with_a_reference_to_a_wrong_kind_names_the_valid_kinds() {
    let mut session = session(&two_sites());
    let refusal = refused(&mut session, add("a-earth", arc("a", "earth"), None));
    assert_eq!(
        refusal,
        Refusal::WrongKind {
            field: "to",
            target: id("earth"),
            kind: Kind::Globe,
            accepts: &[Kind::Building],
        }
    );
    assert_eq!(
        refusal.to_string(),
        "to: earth is a globe, valid kinds: building"
    );

    let refusal = refused(&mut session, add("l", label("a-b.label"), None));
    assert_eq!(
        refusal.to_string(),
        "of: a-b.label is a label, valid kinds: globe, building, arc"
    );
}

#[test]
fn arc_from_a_building_to_itself_is_refused() {
    let mut session = session(&two_sites());
    let refusal = refused(&mut session, add("a-a", arc("a", "a"), None));
    assert_eq!(refusal, Refusal::SameEnds { object: id("a") });
    assert_eq!(refusal.to_string(), "from and to are the same object a");
}

#[test]
fn remove_of_no_object_is_refused() {
    let mut session = session(&two_sites());
    let refusal = refused(&mut session, remove("c"));
    assert_eq!(refusal, Refusal::NoObject { id: id("c") });
    assert_eq!(refusal.to_string(), "no object c");
}

#[test]
fn remove_of_an_object_in_use_names_its_users_in_name_order() {
    let mut session = session(&two_sites());
    let refusal = refused(&mut session, remove("earth"));
    assert_eq!(
        refusal,
        Refusal::InUse {
            id: id("earth"),
            by: vec![id("a"), id("b")]
        }
    );
    assert_eq!(refusal.to_string(), "earth is in use by a, b");
}

#[test]
fn replay_gives_the_index_of_the_refused_operation() {
    let mut log = two_sites();
    log.insert(3, remove("earth"));
    let (index, refusal) = Session::replay(&log).unwrap_err();
    assert_eq!(index, 3);
    assert_eq!(refusal.to_string(), "earth is in use by a, b");
}

#[test]
fn add_each_gives_one_object_with_the_members_in_the_order_of_the_host() {
    let session = sites_with_flows();
    assert_eq!(session.scene().len(), 4);
    let object = session.scene().get(&id("top")).unwrap();
    assert_eq!(object.primitive, arc("a", "b"));
    assert_eq!(
        object.binding,
        Binding::Each {
            set: Handle::new("flows:a-b"),
            members: handles(&["flow:7", "flow:2", "flow:5"]),
        }
    );
}

#[test]
fn scene_text_of_a_set_object_has_its_set_and_its_member_count() {
    let mut session = sites_with_flows();
    session
        .add_each(id("back"), arc("b", "a"), Handle::new("flows:b-a"), &host())
        .unwrap();
    assert_eq!(
        session.scene().describe(),
        "a building on=earth handle=\"site:a\"\n\
         b building on=earth handle=\"site:b\"\n\
         back arc from=b to=a set=\"flows:b-a\" members=0\n\
         earth globe\n\
         top arc from=a to=b set=\"flows:a-b\" members=3\n"
    );
}

#[test]
fn add_each_with_a_handle_that_is_no_set_of_the_host_is_refused() {
    let mut session = session(&two_sites()[..3]);
    let before = session.clone();
    let refusal = session
        .add_each(id("top"), arc("a", "b"), Handle::new("flows:a-c"), &host())
        .unwrap_err();
    assert_eq!(session, before);
    assert_eq!(
        refusal,
        Refusal::NoSet {
            set: Handle::new("flows:a-c")
        }
    );
    assert_eq!(refusal.to_string(), "no set \"flows:a-c\"");
}

#[test]
fn add_each_has_the_refusals_of_add() {
    let mut session = sites_with_flows();
    let before = session.clone();
    let set = Handle::new("flows:a-b");

    let refusal = session
        .add_each(id("a"), arc("a", "b"), set.clone(), &host())
        .unwrap_err();
    assert_eq!(refusal.to_string(), "object a exists");

    let refusal = session
        .add_each(id("more"), arc("a", "earth"), set.clone(), &host())
        .unwrap_err();
    assert_eq!(
        refusal.to_string(),
        "to: earth is a globe, valid kinds: building"
    );

    let refusal = session
        .add_each(id("more"), arc("a", "a"), set, &host())
        .unwrap_err();
    assert_eq!(refusal.to_string(), "from and to are the same object a");
    assert_eq!(session, before);
}

#[test]
fn reference_to_a_set_object_is_refused() {
    let mut session = sites_with_flows();
    let refusal = refused(&mut session, add("top.label", label("top"), None));
    assert_eq!(
        refusal,
        Refusal::SetTarget {
            field: "of",
            target: id("top")
        }
    );
    assert_eq!(refusal.to_string(), "of: top is a set object");
}

#[test]
fn replay_of_a_log_with_a_set_object_gives_the_same_session() {
    let first = sites_with_flows();
    let second = session(first.log());
    assert_eq!(second, first);
    assert_eq!(second.scene().describe(), first.scene().describe());
}

#[test]
fn undo_of_an_add_each_removes_the_set_object() {
    let mut session = sites_with_flows();
    assert!(matches!(session.undo(), Some(Operation::AddEach { .. })));
    assert_eq!(session, self::session(&two_sites()[..3]));
}

#[test]
fn undo_of_a_remove_restores_the_set_object_with_its_members() {
    let mut session = sites_with_flows();
    let before = session.clone();
    session.apply(remove("top")).unwrap();
    assert_eq!(session.scene().get(&id("top")), None);
    assert_eq!(session.undo(), Some(remove("top")));
    assert_eq!(session, before);
}
