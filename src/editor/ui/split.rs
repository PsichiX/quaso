use raui_core::widget::unit::flex::FlexBoxItemLayout;
use raui_immediate::{begin, end, push};
use raui_immediate_widgets::core::containers::{horizontal_box, vertical_box};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitSideSize {
    Fixed(f32),
    Weight(f32),
}

impl Default for SplitSideSize {
    fn default() -> Self {
        Self::Weight(1.0)
    }
}

impl SplitSideSize {
    fn into_layout(self) -> FlexBoxItemLayout {
        match self {
            Self::Fixed(v) => FlexBoxItemLayout {
                basis: Some(v),
                grow: 0.0,
                shrink: 0.0,
                ..Default::default()
            },
            Self::Weight(v) => FlexBoxItemLayout {
                grow: v,
                shrink: v,
                ..Default::default()
            },
        }
    }
}

pub fn split_horizontal<R>(
    left_size: SplitSideSize,
    right_size: SplitSideSize,
    mut f: impl FnMut() -> R,
) -> R {
    horizontal_box((), || {
        begin();
        let result = f();
        let widgets = end();
        for (mut widget, size) in widgets
            .into_iter()
            .take(2)
            .zip([left_size, right_size].into_iter())
        {
            widget
                .props_mut()
                .unwrap()
                .merge_from(size.into_layout().into());
            push(widget);
        }
        result
    })
}

pub fn split_vertical<R>(
    top_size: SplitSideSize,
    bottom_size: SplitSideSize,
    mut f: impl FnMut() -> R,
) -> R {
    vertical_box((), || {
        begin();
        let result = f();
        let widgets = end();
        for (mut widget, size) in widgets
            .into_iter()
            .take(2)
            .zip([top_size, bottom_size].into_iter())
        {
            widget
                .props_mut()
                .unwrap()
                .merge_from(size.into_layout().into());
            push(widget);
        }
        result
    })
}
