use std::ops::Range;

use gpui::{
    AnyElement, App, Bounds, Context, ElementId, ElementInputHandler, EntityInputHandler,
    FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels,
    Point, RenderOnce, Styled, UTF16Selection, Window, canvas, div, prelude::FluentBuilder, px,
};
use silon_gpui::Dispatcher;

use crate::{Space, Theme};

/// One key press, as plain data a view can turn into an action.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyPress {
    /// The key's name: `a`, `enter`, `up`, `f1`, ...
    pub key: String,
    /// The text the press types, with Shift and the layout applied.
    pub text: Option<String>,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Cmd on macOS, the Windows or Super key elsewhere.
    pub platform: bool,
}

impl KeyPress {
    /// Whether the press only types its text: printable text with no
    /// modifier but Shift. Such presses go to text input, where an input
    /// method may compose them.
    pub fn is_text(&self) -> bool {
        let printable = self
            .text
            .as_deref()
            .is_some_and(|text| !text.is_empty() && !text.chars().any(char::is_control));
        printable && !self.ctrl && !self.alt && !self.platform
    }

    /// The platform's paste shortcut: Cmd-V on macOS, Ctrl-Shift-V or
    /// Shift-Insert elsewhere.
    pub fn is_paste(&self) -> bool {
        let v = self.key.eq_ignore_ascii_case("v");
        if cfg!(target_os = "macos") {
            self.platform && v && !self.ctrl && !self.alt
        } else {
            (self.ctrl && self.shift && v && !self.alt)
                || (self.shift && self.key == "insert" && !self.ctrl && !self.alt)
        }
    }
}

impl From<&gpui::Keystroke> for KeyPress {
    fn from(keystroke: &gpui::Keystroke) -> Self {
        let modifiers = keystroke.modifiers;
        Self {
            key: keystroke.key.clone(),
            text: keystroke.key_char.clone(),
            ctrl: modifiers.control,
            alt: modifiers.alt,
            shift: modifiers.shift,
            platform: modifiers.platform,
        }
    }
}

/// An area that takes keyboard focus when clicked and reports each key
/// pressed while focused. The focus handle is the element's own ephemeral
/// state; the view only sees [`KeyPress`]es.
#[derive(IntoElement)]
pub struct KeyCapture {
    id: ElementId,
    on_key: Option<Dispatcher<KeyPress>>,
    on_text: Option<Dispatcher<String>>,
    on_paste: Option<Dispatcher<String>>,
    autofocus: bool,
    grow: bool,
    children: Vec<AnyElement>,
}

impl KeyCapture {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            on_key: None,
            on_text: None,
            on_paste: None,
            autofocus: false,
            grow: false,
            children: Vec::new(),
        }
    }

    /// Dispatches every key pressed while the area has focus, except text
    /// when [`Self::on_text`] is set.
    pub fn on_key(mut self, dispatcher: Dispatcher<KeyPress>) -> Self {
        self.on_key = Some(dispatcher);
        self
    }

    /// Takes typed text through the platform's text input, so input
    /// methods (Japanese, Chinese, dead keys) work: dispatches each piece
    /// of committed text, and shows the text being composed meanwhile.
    pub fn on_text(mut self, dispatcher: Dispatcher<String>) -> Self {
        self.on_text = Some(dispatcher);
        self
    }

    /// On the platform's paste shortcut, dispatches the clipboard's text
    /// instead of the key.
    pub fn on_paste(mut self, dispatcher: Dispatcher<String>) -> Self {
        self.on_paste = Some(dispatcher);
        self
    }

    /// Takes focus when it is drawn and nothing else in the window has it.
    pub fn autofocus(mut self) -> Self {
        self.autofocus = true;
        self
    }

    /// Takes the remaining space of its parent.
    pub fn grow(mut self) -> Self {
        self.grow = true;
        self
    }
}

impl ParentElement for KeyCapture {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

/// Text an input method is composing, before it is committed.
#[derive(Debug, Default, PartialEq, Eq)]
struct Composition {
    marked: Option<String>,
}

impl Composition {
    fn utf16_len(&self) -> usize {
        self.marked
            .as_deref()
            .map_or(0, |m| m.encode_utf16().count())
    }

    /// The input method replaced the composed text with `text`, still
    /// composing.
    fn mark(&mut self, text: &str) {
        self.marked = (!text.is_empty()).then(|| text.to_owned());
    }

    /// The input method committed `text`; returns what to dispatch.
    fn commit(&mut self, text: &str) -> Option<String> {
        self.marked = None;
        (!text.is_empty()).then(|| text.to_owned())
    }
}

/// The element's own state across frames: its focus, and what is being
/// composed.
struct CaptureState {
    focus: FocusHandle,
    composition: Composition,
    on_text: Option<Dispatcher<String>>,
}

impl EntityInputHandler for CaptureState {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let marked = self.composition.marked.as_deref().unwrap_or("");
        let utf16: Vec<u16> = marked.encode_utf16().collect();
        let end = range.end.min(utf16.len());
        let start = range.start.min(end);
        *adjusted = Some(start..end);
        Some(String::from_utf16_lossy(&utf16[start..end]))
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.composition.utf16_len();
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.composition
            .marked
            .as_ref()
            .map(|_| 0..self.composition.utf16_len())
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.composition.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let (Some(text), Some(on_text)) = (self.composition.commit(text), &self.on_text) {
            on_text.dispatch(text, cx);
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _selected: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composition.mark(text);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // Candidates open under the area's top edge; the area does not know
        // where its content draws a caret.
        Some(Bounds::new(
            element_bounds.origin,
            gpui::size(px(1.), px(24.)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl RenderOnce for KeyCapture {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| CaptureState {
            focus: cx.focus_handle(),
            composition: Composition::default(),
            on_text: None,
        });
        let takes_text = self.on_text.is_some();
        let on_text = self.on_text;
        state.update(cx, |state, _| state.on_text = on_text);
        let (focus, marked) = {
            let state = state.read(cx);
            (state.focus.clone(), state.composition.marked.clone())
        };
        if self.autofocus && window.focused(cx).is_none() {
            let focus = focus.clone();
            window.defer(cx, move |window, _| window.focus(&focus));
        }
        let theme = Theme::of(cx);
        let border = if focus.is_focused(window) {
            theme.colors.accent
        } else {
            theme.colors.border
        };
        let on_key = self.on_key;
        let on_paste = self.on_paste;
        let clicked: FocusHandle = focus.clone();
        let input_focus = focus.clone();
        // Registers the text input while painting, as GPUI requires.
        let text_input = takes_text.then(|| {
            canvas(
                |_, _, _| {},
                move |bounds, (), window, cx| {
                    window.handle_input(&input_focus, ElementInputHandler::new(bounds, state), cx);
                },
            )
            .absolute()
            .size_full()
        });
        let composing = marked.map(|marked| {
            div()
                .absolute()
                .bottom(theme.space(Space::Xs))
                .left(theme.space(Space::Md))
                .px(theme.space(Space::Xs))
                .rounded(theme.radius_sm)
                .bg(theme.colors.surface)
                .border_1()
                .border_color(theme.colors.accent)
                .text_color(theme.colors.text)
                .underline()
                .child(marked)
        });

        div()
            .id(self.id)
            .relative()
            .track_focus(&focus)
            .on_mouse_down(MouseButton::Left, move |_, window, _| {
                window.focus(&clicked);
            })
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                let press = KeyPress::from(&event.keystroke);
                if press.key.is_empty() && press.text.is_none() {
                    // Names nothing and types nothing.
                    return;
                }
                if let (Some(on_paste), true) = (&on_paste, press.is_paste()) {
                    let text = cx.read_from_clipboard().and_then(|item| item.text());
                    if let Some(text) = text {
                        on_paste.dispatch(text, cx);
                    }
                    cx.stop_propagation();
                } else if takes_text && press.is_text() {
                    // Left to propagate: the platform hands it to the text
                    // input, through the input method if one is active.
                } else if let Some(on_key) = &on_key {
                    on_key.dispatch(press, cx);
                    cx.stop_propagation();
                }
            })
            .when(self.grow, |d| d.flex_1().min_w_0().min_h_0())
            .p(theme.space(Space::Md))
            .bg(theme.colors.background)
            .border_1()
            .border_color(border)
            .rounded(theme.radius_md)
            .children(text_input)
            .children(self.children)
            .children(composing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composing_then_committing_dispatches_only_the_commit() {
        let mut composition = Composition::default();
        composition.mark("に");
        composition.mark("にほ");
        assert_eq!(composition.marked.as_deref(), Some("にほ"));
        assert_eq!(composition.utf16_len(), 2);

        assert_eq!(composition.commit("日本"), Some("日本".into()));
        assert_eq!(composition, Composition::default());
    }

    #[test]
    fn an_empty_mark_or_commit_ends_the_composition_quietly() {
        let mut composition = Composition::default();
        composition.mark("a");
        composition.mark("");
        assert_eq!(composition.marked, None);

        composition.mark("a");
        assert_eq!(composition.commit(""), None);
        assert_eq!(composition.marked, None);
    }

    #[test]
    fn only_unmodified_printable_presses_are_text() {
        let press = |key: &str, text: Option<&str>| KeyPress {
            key: key.into(),
            text: text.map(Into::into),
            ..KeyPress::default()
        };
        assert!(press("a", Some("a")).is_text());
        assert!(press("space", Some(" ")).is_text());
        let shifted = KeyPress {
            shift: true,
            ..press("a", Some("A"))
        };
        assert!(shifted.is_text());

        assert!(!press("enter", Some("\n")).is_text());
        assert!(!press("tab", Some("\t")).is_text());
        assert!(!press("backspace", None).is_text());
        assert!(!press("a", Some("")).is_text());
        for modified in [
            KeyPress {
                ctrl: true,
                ..press("c", Some("c"))
            },
            KeyPress {
                alt: true,
                ..press("b", Some("b"))
            },
            KeyPress {
                platform: true,
                ..press("v", Some("v"))
            },
        ] {
            assert!(!modified.is_text(), "{modified:?}");
        }
    }
}
