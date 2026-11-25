use gpui::{App, ElementId, IntoElement, ParentElement, Window};
use silon_gpui::{Dispatcher, FeatureView};
use silon_ui::components::TextInput;
use silon_ui::{
    Align, Badge, Button, ButtonVariant, Checkbox, Justify, Panel, Row, Space, Stack, Text,
    TextSize, Tone,
};
use todo_core::{TodoAction, TodoFeature, TodoItem, TodoState};

pub struct TodoView;

impl FeatureView<TodoFeature> for TodoView {
    fn view(
        state: &TodoState,
        dispatcher: Dispatcher<TodoAction>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> impl IntoElement + use<> {
        let done = state.items.len() - state.remaining();

        Stack::screen()
            .padding(Space::Xl)
            .gap(Space::Lg)
            .child(
                Row::new()
                    .gap(Space::Md)
                    .child(Text::new("Todo").size(TextSize::Title))
                    .child(Badge::new(format!("{} left", state.remaining())).tone(Tone::Accent)),
            )
            .child(
                Row::new()
                    .gap(Space::Sm)
                    .child(
                        Stack::new().grow().child(
                            TextInput::new("draft", state.draft.clone())
                                .placeholder("What needs to be done?")
                                .on_change(dispatcher.map(TodoAction::DraftChanged))
                                .on_submit(dispatcher.map(|_: String| TodoAction::Submit)),
                        ),
                    )
                    .child(
                        Button::new("add", "Add")
                            .variant(ButtonVariant::Primary)
                            .disabled(state.draft.trim().is_empty())
                            .on_press(&dispatcher, TodoAction::Submit),
                    ),
            )
            .child(items(&state.items, &dispatcher))
            .child(
                Row::new()
                    .justify(Justify::Between)
                    .child(
                        Text::new(format!("{} items, {done} done", state.items.len()))
                            .size(TextSize::Caption)
                            .tone(Tone::Muted),
                    )
                    .child(
                        Button::new("clear-completed", "Clear completed")
                            .variant(ButtonVariant::Ghost)
                            .disabled(done == 0)
                            .on_press(&dispatcher, TodoAction::ClearCompleted),
                    ),
            )
    }
}

fn items(items: &[TodoItem], dispatcher: &Dispatcher<TodoAction>) -> Panel {
    let panel = Panel::new().gap(Space::Sm);
    if items.is_empty() {
        return panel.child(Text::new("Nothing to do.").tone(Tone::Muted));
    }
    panel.children(items.iter().map(|item| {
        Row::new()
            .gap(Space::Md)
            .align(Align::Center)
            .child(
                Checkbox::new(ElementId::NamedInteger("toggle".into(), item.id), item.done)
                    .on_toggle(dispatcher, TodoAction::Toggle(item.id)),
            )
            .child(
                Stack::new().grow().child(
                    Text::new(item.title.clone())
                        .tone(if item.done {
                            Tone::Muted
                        } else {
                            Tone::Neutral
                        })
                        .strikethrough(item.done),
                ),
            )
            .child(
                Button::new(ElementId::NamedInteger("remove".into(), item.id), "Remove")
                    .variant(ButtonVariant::Ghost)
                    .on_press(dispatcher, TodoAction::Remove(item.id)),
            )
    }))
}
