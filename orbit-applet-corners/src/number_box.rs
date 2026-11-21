// SPDX-License-Identifier: GPL-3.0-only

//! A small number field: type a value and press Enter, or nudge it with the
//! inline up and down arrows. libcosmic's spin button can't be typed into.

use cosmic::{
    Element,
    iced::{Alignment, Length},
    widget::{button, column, icon, text_input},
};

/// What a draft means once parsed. Empty or junk keeps the old value.
/// `pill` is what typing "pill" gives, where a pill is allowed.
pub fn parse(draft: &str, min: u32, max: u32, pill: Option<u32>) -> Option<u32> {
    let draft = draft.trim();
    if draft.eq_ignore_ascii_case("pill") {
        return pill;
    }
    let digits = draft.trim_end_matches("px").trim();
    digits.parse::<u32>().ok().map(|v| v.clamp(min, max))
}

/// With `on_input` set to `None` the box and its arrows are disabled.
pub struct NumberBox<M> {
    pub on_input: Option<Box<dyn Fn(String) -> M>>,
    pub on_commit: M,
    pub on_up: Option<M>,
    pub on_down: Option<M>,
}

pub fn number_box<'a, M: Clone + 'static>(text: String, msgs: NumberBox<M>) -> Element<'a, M> {
    let arrow = |name: &'static str, msg: Option<M>| {
        button::custom(icon::from_name(name).size(10).icon())
            .padding(0)
            .class(cosmic::theme::Button::Icon)
            .on_press_maybe(msg)
    };
    let arrows = column::with_children(vec![
        arrow("pan-up-symbolic", msgs.on_up).into(),
        arrow("pan-down-symbolic", msgs.on_down).into(),
    ])
    .align_x(Alignment::Center);

    let NumberBox {
        on_input,
        on_commit,
        ..
    } = msgs;
    let mut input = text_input::text_input("", text)
        .trailing_icon(arrows.into())
        .width(Length::Fixed(84.0));
    if let Some(on_input) = on_input {
        let commit = on_commit.clone();
        input = input
            .on_input(on_input)
            .on_submit(move |_| commit.clone())
            .on_unfocus(on_commit);
    }
    input.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_values_are_clamped_and_tolerate_units() {
        assert_eq!(parse("16", 0, 32, None), Some(16));
        assert_eq!(parse(" 20 px", 0, 32, None), Some(20));
        assert_eq!(parse("99", 0, 32, None), Some(32));
        assert_eq!(parse("", 0, 32, None), None);
        assert_eq!(parse("abc", 0, 32, None), None);
        assert_eq!(parse("Pill", 0, 32, Some(160)), Some(160));
        assert_eq!(parse("pill", 0, 32, None), None);
    }
}
