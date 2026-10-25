//! A small app that exercises every path of the control protocol.
//!
//! ```sh
//! cargo run --example demo                      # windowed
//! cargo run --example demo -- --headless --control /tmp/demo.sock &
//! cargo run --example demo -- ctl /tmp/demo.sock tree
//! ```

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iced::keyboard::Modifiers;
use iced::widget::{
    Space, button, column, container, pick_list, progress_bar, row, scrollable, text, text_input,
};
use iced::{Element, Length, Subscription, Task};

fn main() -> iced::Result {
    let app = iced::application(Demo::new, Demo::update, Demo::view)
        .subscription(Demo::subscription)
        .window_size((1100.0, 720.0));
    iced_impeccable::Remote::new(app)
        .heartbeat(|m| matches!(m, Message::Tick | Message::Frame(_)))
        .command("reset", "clear the form", |_| Ok(Message::Reset))
        .run()
}

#[derive(Debug, Clone)]
enum Message {
    Tick,
    Frame(Instant),
    Name(String),
    Save,
    Copy,
    ToggleSettings,
    ToggleSpinner,
    Density(Density),
    Reset,
    Dropped(PathBuf),
    Mods(Modifiers),
    Hotkey,
    Slept,
    Sleep,
    Monitor,
    MonitorSize(Option<iced::Size>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Density {
    Compact,
    Comfortable,
    Spacious,
}

impl Density {
    const ALL: [Density; 3] = [Density::Compact, Density::Comfortable, Density::Spacious];
}

impl std::fmt::Display for Density {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Density::Compact => "Compact",
            Density::Comfortable => "Comfortable",
            Density::Spacious => "Spacious",
        })
    }
}

struct Demo {
    clock: String,
    name: String,
    status: Option<String>,
    settings: bool,
    density: Density,
    /// When the spinner was switched on.
    spinning: Option<Instant>,
    phase: f32,
    /// The modifiers held down, as the keyboard last reported them.
    mods: Modifiers,
}

impl Drop for Demo {
    fn drop(&mut self) {
        eprintln!("demo: state dropped");
    }
}

impl Demo {
    fn new() -> Self {
        Self {
            clock: clock(),
            name: String::new(),
            status: None,
            settings: false,
            density: Density::Comfortable,
            spinning: None,
            phase: 0.0,
            mods: Modifiers::empty(),
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => self.clock = clock(),
            Message::Frame(now) => {
                if let Some(started) = self.spinning {
                    self.phase = (now - started).as_secs_f32().fract();
                }
            }
            Message::Name(name) => self.name = name,
            Message::Save => self.status = Some(format!("Saved {}", self.name)),
            Message::Copy => return iced::clipboard::write(self.name.clone()),
            Message::ToggleSettings => self.settings = !self.settings,
            Message::ToggleSpinner => {
                self.spinning = match self.spinning {
                    Some(_) => None,
                    None => Some(Instant::now()),
                };
                self.phase = 0.0;
            }
            Message::Density(density) => self.density = density,
            Message::Reset => {
                self.name.clear();
                self.status = None;
            }
            Message::Mods(modifiers) => self.mods = modifiers,
            Message::Hotkey => self.status = Some("Hotkey".into()),
            Message::Slept => self.status = Some("Slept".into()),
            Message::Sleep => {
                return Task::perform(
                    async { std::thread::sleep(Duration::from_millis(1500)) },
                    |()| Message::Slept,
                );
            }
            Message::Monitor => {
                return iced::window::oldest()
                    .and_then(iced::window::monitor_size)
                    .map(Message::MonitorSize);
            }
            Message::MonitorSize(size) => {
                self.status = Some(match size {
                    Some(size) => format!("Monitor {}x{}", size.width as u32, size.height as u32),
                    None => "Monitor unknown".to_owned(),
                });
            }
            Message::Dropped(path) => {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                self.status = Some(format!("Dropped {name}"));
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        let tick = iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick);
        let events = iced::event::listen_with(events);
        if self.spinning.is_some() {
            Subscription::batch([tick, events, iced::window::frames().map(Message::Frame)])
        } else {
            Subscription::batch([tick, events])
        }
    }

    fn view(&self) -> Element<'_, Message> {
        // Icon-only: no text to find it by, so it carries an id.
        let settings =
            container(button(Space::new().width(16).height(16)).on_press(Message::ToggleSettings))
                .id("settings");
        let header = row![
            text("Impeccable demo").size(24),
            Space::new().width(Length::Fill),
            text(&self.clock),
            settings,
        ]
        .spacing(16)
        .align_y(iced::Alignment::Center);

        let form = column![
            text("Save").size(20),
            text_input("Your name", &self.name)
                .id("name")
                .on_input(Message::Name)
                .on_submit(Message::Save),
            row![
                button(text("Save")).on_press(Message::Save),
                button(text("Copy")).on_press(Message::Copy),
                button(text("Spinner")).on_press(Message::ToggleSpinner),
                button(text("Sleep")).on_press(Message::Sleep),
                button(text("Monitor")).on_press(Message::Monitor),
            ]
            .spacing(8),
        ]
        .push(text(format!("Mods {}", modifier_names(self.mods))))
        .push(self.status.as_deref().map(text))
        .push(
            self.spinning
                .map(|_| progress_bar(0.0..=1.0, self.phase).length(240)),
        )
        .spacing(12)
        .width(400);

        let rows = scrollable(
            column((1..=60).map(|i| text(format!("Row {i}")).into()))
                .spacing(4)
                .width(Length::Fill),
        )
        .id("rows")
        .height(Length::Fill);
        // The text "#rows" next to the id "rows": `=#rows` finds the text.
        let rows = column![text("#rows"), rows].spacing(4).height(Length::Fill);

        let mut body = row![form, rows].spacing(24).height(Length::Fill);
        if self.settings {
            body = body.push(
                column![
                    text("Settings").size(20),
                    // A pick list reports no text to the inventory.
                    pick_list(Density::ALL, Some(self.density), Message::Density),
                ]
                .spacing(8)
                .width(200),
            );
        }

        container(column![header, body].spacing(24))
            .padding(24)
            .into()
    }
}

/// Window and keyboard events the demo reacts to: a dropped file, the held
/// modifiers, and Ctrl+K by its physical key.
fn events(event: iced::Event, _: iced::event::Status, _: iced::window::Id) -> Option<Message> {
    use iced::keyboard::Event;
    use iced::keyboard::key::{Code, Physical};
    match event {
        iced::Event::Window(iced::window::Event::FileDropped(path)) => Some(Message::Dropped(path)),
        iced::Event::Keyboard(Event::ModifiersChanged(modifiers)) => Some(Message::Mods(modifiers)),
        iced::Event::Keyboard(Event::KeyPressed {
            physical_key: Physical::Code(Code::KeyK),
            modifiers,
            ..
        }) if modifiers.control() => Some(Message::Hotkey),
        _ => None,
    }
}

/// The held modifiers as words, or `none`.
fn modifier_names(modifiers: Modifiers) -> String {
    let names: Vec<&str> = [
        (modifiers.control(), "ctrl"),
        (modifiers.shift(), "shift"),
        (modifiers.alt(), "alt"),
        (modifiers.logo(), "super"),
    ]
    .into_iter()
    .filter_map(|(held, name)| held.then_some(name))
    .collect();
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(" ")
    }
}

/// `HH:MM:SS` UTC.
fn clock() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600 % 24,
        secs / 60 % 60,
        secs % 60
    )
}
