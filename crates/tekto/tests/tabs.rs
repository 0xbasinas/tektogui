use tekto::{
    accesskit_dump, button, layout_tree, paint_tree, tab, tab_list, window, Color, CpuRenderer,
    NodePatch, Renderer, Tree,
};
use tekto_tree::{route_named_key, NamedActionKey};

#[test]
fn tabs_roving_focus_selection_and_semantics() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Settings", 360.0, 180.0).unwrap();
    let list = tab_list(&mut tree, root, "Pages").unwrap();
    let first = tab(&mut tree, list, "General", true).unwrap();
    let disabled = tab(&mut tree, list, "Unavailable", false).unwrap();
    let last = tab(&mut tree, list, "Advanced", false).unwrap();
    let next = button(&mut tree, root, "Save").unwrap();
    tree.patch(
        disabled,
        NodePatch {
            disabled: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    layout_tree(&mut tree, 360.0, 180.0);
    let dump = accesskit_dump(&tree).unwrap();
    assert_eq!(
        dump.nodes.iter().find(|n| n.id == list.raw()).unwrap().role,
        "TabList"
    );
    let node = dump.nodes.iter().find(|n| n.id == first.raw()).unwrap();
    assert_eq!(node.role, "Tab");
    assert_eq!(node.selected, Some(true));
    assert!(tree.traverse_focus(false));
    assert_eq!(tree.focus, Some(first));
    let mut before = CpuRenderer::new(360, 180).unwrap();
    paint_tree(&mut before, &tree, Color::PAGE);
    assert!(route_named_key(&mut tree, NamedActionKey::ArrowRight, false).0);
    assert_eq!(tree.focus, Some(last));
    assert!(!tree.get(first).unwrap().checked);
    assert!(tree.get(last).unwrap().checked);
    let mut after = CpuRenderer::new(360, 180).unwrap();
    paint_tree(&mut after, &tree, Color::PAGE);
    assert_ne!(before.rgba8().unwrap(), after.rgba8().unwrap());
    assert!(tree.traverse_focus(false));
    assert_eq!(tree.focus, Some(next));
    assert!(tree.click(disabled).is_err());
    tree.click(first).unwrap();
    assert!(tree.get(first).unwrap().checked);
    assert!(!tree.get(last).unwrap().checked);
}

#[test]
fn tab_panels_preserve_state_and_exclude_inactive_controls() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Settings", 360.0, 240.0).unwrap();
    let list = tab_list(&mut tree, root, "Pages").unwrap();
    let first = tab(&mut tree, list, "General", true).unwrap();
    let second = tab(&mut tree, list, "Advanced", false).unwrap();
    let first_panel = tekto::tab_panel(&mut tree, root, first, "General panel").unwrap();
    let second_panel = tekto::tab_panel(&mut tree, root, second, "Advanced panel").unwrap();
    let first_button = button(&mut tree, first_panel, "First action").unwrap();
    let second_button = button(&mut tree, second_panel, "Second action").unwrap();
    layout_tree(&mut tree, 360.0, 240.0);
    let dump = accesskit_dump(&tree).unwrap();
    assert_eq!(
        dump.nodes
            .iter()
            .find(|node| node.id == first_panel.raw())
            .unwrap()
            .role,
        "TabPanel"
    );
    assert!(!dump.nodes.iter().any(|node| node.id == second_button.raw()));
    assert!(tree.set_focus(second_button).is_err());
    tree.click(second).unwrap();
    layout_tree(&mut tree, 360.0, 240.0);
    let dump = accesskit_dump(&tree).unwrap();
    assert!(!dump.nodes.iter().any(|node| node.id == first_button.raw()));
    assert!(dump.nodes.iter().any(|node| node.id == second_button.raw()));
    assert!(tree.set_focus(first_button).is_err());
    tree.set_focus(second_button).unwrap();
    tree.patch(
        first,
        NodePatch {
            checked: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(tree.focus, Some(first));
    assert!(tree.get(second_button).is_some());
    tree.remove(first).unwrap();
    assert!(tree.get(first_panel).is_none());
}

#[test]
fn tab_panels_survive_removing_the_selected_tab_and_adding_new_ones() {
    let mut tree = Tree::new();
    let root = window(&mut tree, "Settings", 360.0, 240.0).unwrap();
    let list = tab_list(&mut tree, root, "Pages").unwrap();
    let first = tab(&mut tree, list, "One", true).unwrap();
    let second = tab(&mut tree, list, "Two", false).unwrap();
    let first_panel = tekto::tab_panel(&mut tree, root, first, "One panel").unwrap();
    let second_panel = tekto::tab_panel(&mut tree, root, second, "Two panel").unwrap();
    let kept = button(&mut tree, second_panel, "Kept").unwrap();
    layout_tree(&mut tree, 360.0, 240.0);
    // Remove the selected tab: its panel goes, the other panel stays inactive.
    tree.remove(first).unwrap();
    assert!(tree.get(first_panel).is_none());
    assert!(tree.get(second_panel).is_some());
    assert!(tree.get(kept).is_some());
    // A tab added later gets its own panel and can be selected.
    let third = tab(&mut tree, list, "Three", false).unwrap();
    let third_panel = tekto::tab_panel(&mut tree, root, third, "Three panel").unwrap();
    let late = button(&mut tree, third_panel, "Late").unwrap();
    tree.click(third).unwrap();
    layout_tree(&mut tree, 360.0, 240.0);
    let dump = accesskit_dump(&tree).unwrap();
    assert!(dump.nodes.iter().any(|n| n.id == late.raw()));
    assert!(!dump.nodes.iter().any(|n| n.id == kept.raw()));
    // Binding a panel twice or to a tab that already has one is refused.
    assert!(tree.bind_tab_panel(third, second_panel).is_err());
    // Removing a panel directly leaves the tab usable.
    tree.remove(third_panel).unwrap();
    tree.click(second).unwrap();
    let dump = accesskit_dump(&tree).unwrap();
    assert!(dump.nodes.iter().any(|n| n.id == kept.raw()));
}
