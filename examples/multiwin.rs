//! A daemon with two windows, for the smoke test of window selection: the
//! main window opens a second one, which closes itself.

use iced::widget::{button, column, text};
use iced::{Element, Size, Subscription, Task, window};

fn main() -> iced::Result {
    let app = iced::daemon(boot, update, view)
        .title(title)
        .subscription(subscription);
    iced_impeccable::Remote::new(app).run()
}

#[derive(Default)]
struct State {
    main: Option<window::Id>,
    second: Option<window::Id>,
    /// The window that last gained focus and has not lost it.
    focused: Option<window::Id>,
}

#[derive(Debug, Clone)]
enum Message {
    MainOpened(window::Id),
    OpenSecond,
    SecondOpened(window::Id),
    Close(window::Id),
    CloseRequested(window::Id),
    Closed(window::Id),
    Focus(window::Id, bool),
}

fn boot() -> (State, Task<Message>) {
    let (_, open) = window::open(window::Settings {
        size: Size::new(400.0, 300.0),
        exit_on_close_request: false,
        ..window::Settings::default()
    });
    (State::default(), open.map(Message::MainOpened))
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::MainOpened(id) => {
            state.main = Some(id);
            Task::none()
        }
        Message::OpenSecond if state.second.is_none() => {
            let (_, open) = window::open(window::Settings {
                size: Size::new(320.0, 240.0),
                ..window::Settings::default()
            });
            open.map(Message::SecondOpened)
        }
        Message::OpenSecond => Task::none(),
        Message::SecondOpened(id) => {
            state.second = Some(id);
            Task::none()
        }
        Message::Close(id) => window::close(id),
        Message::CloseRequested(id) if Some(id) == state.main => iced::exit(),
        Message::CloseRequested(id) => window::close(id),
        Message::Closed(id) => {
            if state.second == Some(id) {
                state.second = None;
            }
            Task::none()
        }
        Message::Focus(id, true) => {
            state.focused = Some(id);
            Task::none()
        }
        // The window that lost focus only clears it if it still held it.
        Message::Focus(id, false) => {
            if state.focused == Some(id) {
                state.focused = None;
            }
            Task::none()
        }
    }
}

fn view(state: &State, id: window::Id) -> Element<'_, Message> {
    if Some(id) == state.second {
        column![text("second"), button("Close").on_press(Message::Close(id))]
    } else {
        column![
            text("main"),
            text(format!(
                "main focused: {}",
                state.focused.is_some() && state.focused == state.main
            )),
            button("Open second").on_press(Message::OpenSecond)
        ]
    }
    .spacing(12)
    .padding(16)
    .into()
}

fn title(state: &State, id: window::Id) -> String {
    if Some(id) == state.second {
        "multiwin second".to_owned()
    } else {
        "multiwin main".to_owned()
    }
}

fn subscription(_: &State) -> Subscription<Message> {
    Subscription::batch([
        window::close_requests().map(Message::CloseRequested),
        window::close_events().map(Message::Closed),
        iced::event::listen_with(|event, _, id| match event {
            iced::Event::Window(window::Event::Focused) => Some(Message::Focus(id, true)),
            iced::Event::Window(window::Event::Unfocused) => Some(Message::Focus(id, false)),
            _ => None,
        }),
    ])
}
