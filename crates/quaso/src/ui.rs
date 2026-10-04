use crate::context::GameContext;
use raui_core::{
    layout::CoordsMapping,
    widget::{unit::WidgetUnit, utils::Rect},
};
use serde_json::{Value, json};

pub fn screen_area(context: &GameContext) -> Rect {
    let size = context.graphics.state.main_camera.screen_size;
    Rect {
        left: 0.0,
        right: size.x,
        top: 0.0,
        bottom: size.y,
    }
}

pub fn screen_mapping(context: &GameContext) -> CoordsMapping {
    CoordsMapping::new_scaling(screen_area(context), context.gui.coords_map_scaling)
}

pub fn tree(context: &GameContext, max_depth: Option<usize>) -> Value {
    let mapping = screen_mapping(context);
    let root = context.gui.application.rendered_tree();
    json!({
        "screen": rect_to_json(screen_area(context)),
        "root": node_to_json(context, &mapping, root, 0, max_depth),
    })
}

pub fn texts(context: &GameContext) -> Value {
    let mapping = screen_mapping(context);
    let mut found = Vec::new();
    collect_texts(
        context,
        &mapping,
        context.gui.application.rendered_tree(),
        &mut found,
    );
    json!({ "texts": found })
}

fn kind_of(unit: &WidgetUnit) -> &'static str {
    match unit {
        WidgetUnit::None => "None",
        WidgetUnit::AreaBox(_) => "AreaBox",
        WidgetUnit::PortalBox(_) => "PortalBox",
        WidgetUnit::ContentBox(_) => "ContentBox",
        WidgetUnit::FlexBox(_) => "FlexBox",
        WidgetUnit::GridBox(_) => "GridBox",
        WidgetUnit::SizeBox(_) => "SizeBox",
        WidgetUnit::ImageBox(_) => "ImageBox",
        WidgetUnit::TextBox(_) => "TextBox",
    }
}

fn rect_to_json(rect: Rect) -> Value {
    json!({
        "left": rect.left,
        "top": rect.top,
        "right": rect.right,
        "bottom": rect.bottom,
    })
}

fn screen_rect(context: &GameContext, mapping: &CoordsMapping, unit: &WidgetUnit) -> Option<Rect> {
    let id = unit.as_data()?.id();
    let item = context.gui.application.layout_data().items.get(id)?;
    Some(mapping.virtual_to_real_rect(item.ui_space, false))
}

fn node_to_json(
    context: &GameContext,
    mapping: &CoordsMapping,
    unit: &WidgetUnit,
    depth: usize,
    max_depth: Option<usize>,
) -> Value {
    let Some(data) = unit.as_data() else {
        return Value::Null;
    };
    let mut node = json!({
        "id": data.id().to_string(),
        "kind": kind_of(unit),
    });
    if let Some(rect) = screen_rect(context, mapping, unit) {
        node["rect"] = rect_to_json(rect);
    }
    if let WidgetUnit::TextBox(text_box) = unit {
        node["text"] = json!(text_box.text);
    }
    let children = data.get_children();
    if !children.is_empty() {
        if max_depth.is_some_and(|max_depth| depth >= max_depth) {
            node["children_not_shown"] = json!(children.len());
        } else {
            node["children"] = Value::Array(
                children
                    .into_iter()
                    .map(|child| node_to_json(context, mapping, child, depth + 1, max_depth))
                    .filter(|child| !child.is_null())
                    .collect(),
            );
        }
    }
    node
}

fn collect_texts(
    context: &GameContext,
    mapping: &CoordsMapping,
    unit: &WidgetUnit,
    found: &mut Vec<Value>,
) {
    let Some(data) = unit.as_data() else {
        return;
    };
    if let WidgetUnit::TextBox(text_box) = unit
        && !text_box.text.is_empty()
    {
        let mut entry = json!({
            "id": data.id().to_string(),
            "text": text_box.text,
        });
        if let Some(rect) = screen_rect(context, mapping, unit) {
            entry["rect"] = rect_to_json(rect);
        }
        found.push(entry);
    }
    for child in data.get_children() {
        collect_texts(context, mapping, child, found);
    }
}
