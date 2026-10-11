//! The headless host: one thread owns the program state, builds its
//! interface offscreen, feeds it the input the control channel asks for, and
//! answers the runtime's actions the way a window would.
//!
//! Tasks and subscriptions run on the program's executor exactly as in a
//! windowed run, and every input event is broadcast to the subscriptions, so
//! shortcuts registered through `iced::event::listen_with` fire here too.
//! Screenshots are drawn with the real cursor, so hover states show; a
//! recording additionally paints the pointer.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use iced::advanced::clipboard::{self, Clipboard};
use iced::advanced::graphics::text::font_system;
use iced::advanced::renderer::{self, Headless};
use iced::advanced::widget::Operation;
use iced::advanced::widget::operation::{self, Outcome};
use iced::keyboard::Modifiers;
use iced::theme::{Base, Mode};
use iced::{Event, Point, Program, Rectangle, Size, event, mouse};
use iced_futures::futures::StreamExt;
use iced_futures::futures::channel::mpsc as futures_mpsc;
use iced_futures::{Executor, Runtime, subscription};
use iced_runtime::core::window;
use iced_runtime::{Action, Task, UserInterface, task, user_interface};
use iced_selector::Selector;

use crate::args::HostArgs;
use crate::channel::{self, Inflight};
use crate::image::{self, Frame};
use crate::keys::{Keystroke, key_down_events, key_events, key_up_events};
use crate::protocol::{self, Command, Request, Shot};
use crate::target::{self, Node, Target};

/// The frame interval while the interface asks for the next frame.
const FRAME: Duration = Duration::from_millis(16);
/// How long an input command waits for the loop to go quiet before it
/// replies. A shortcut reaches the app through a subscription, which answers
/// asynchronously; without this the next command could overtake it.
const SETTLE: Duration = Duration::from_millis(10);
/// The most an input command waits for that quiet.
const SETTLE_MAX: Duration = Duration::from_millis(250);
/// When `wait-idle` gives up.
const IDLE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long only redraws may keep `wait-idle` busy before it answers
/// `ok animating`.
const ANIMATING_AFTER: Duration = Duration::from_secs(1);
/// How long after an update the interface still counts as redrawing: a
/// `window::frames()` loop answers each frame with a message, and between the
/// frame and that message no redraw is pending.
const UPDATE_GAP: Duration = Duration::from_millis(50);
/// How often `wait-for` and `wait-gone` look at the interface.
const TARGET_POLL: Duration = Duration::from_millis(50);
/// How long `quit` lets an app that handles close requests itself take.
const QUIT_GRACE: Duration = Duration::from_secs(5);
/// How long the exit waits for connections to write their last reply.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
/// Frame rate of `record`.
const RECORD_FPS: u32 = 30;

/// Turns the rest of a custom command's line into a message.
pub(crate) type Build<M> = Box<dyn Fn(&str) -> Result<M, String>>;

/// Tells periodic messages apart from activity.
pub(crate) type Heartbeat<M> = Box<dyn Fn(&M) -> bool>;

/// A command an app registered with `Remote::command`.
pub(crate) struct Custom<M> {
    pub(crate) name: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) build: Build<M>,
}

/// What the app adds to the host.
pub(crate) struct Hooks<M> {
    pub(crate) heartbeat: Heartbeat<M>,
    pub(crate) commands: Vec<Custom<M>>,
}

type HostRuntime<A> = Runtime<
    <A as Program>::Executor,
    futures_mpsc::Sender<HostEvent<<A as Program>::Message>>,
    HostEvent<<A as Program>::Message>,
>;

/// What the executor hands back: a subscription's message or a task's action.
enum HostEvent<M> {
    Message(M),
    Action(Action<M>),
}

/// Everything the loop waits on.
enum Input<M> {
    Host(HostEvent<M>),
    Control(Request, mpsc::Sender<String>),
}

/// Runs the program headless until it exits, then ends the process.
pub(crate) fn run<A>(program: A, hooks: Hooks<A::Message>, args: HostArgs) -> !
where
    A: Program + 'static,
{
    let executor = match <A::Executor as Executor>::new() {
        Ok(executor) => executor,
        Err(e) => {
            eprintln!("[remote] executor: {e}");
            std::process::exit(1);
        }
    };
    let settings = program.settings();
    for font in &settings.fonts {
        font_system()
            .write()
            .expect("font system")
            .load_font(font.clone());
    }
    // No fallback to another backend: a software renderer would hide exactly
    // the pipelines a screenshot is taken to check.
    let renderer = Executor::block_on(
        &executor,
        <A::Renderer as Headless>::new(
            settings.default_font,
            settings.default_text_size,
            Some(args.backend),
        ),
    );
    let Some(renderer) = renderer else {
        eprintln!("[remote] no {} renderer available", args.backend);
        std::process::exit(1);
    };

    let listener = match channel::bind(&args.control) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("[remote] cannot listen on {}: {e}", args.control.display());
            std::process::exit(1);
        }
    };

    let (inputs_tx, inputs) = mpsc::channel();
    let (sender, mut receiver) = futures_mpsc::channel(64);
    let forward = inputs_tx.clone();
    Executor::spawn(&executor, async move {
        while let Some(event) = receiver.next().await {
            if forward.send(Input::Host(event)).is_err() {
                break;
            }
        }
    });
    let inflight = Arc::new(Inflight::default());
    let answer: channel::Answer = Arc::new(move |line: &str| answer(line, &inputs_tx));
    listener.spawn(answer, inflight.clone());

    let runtime = Runtime::new(executor, sender);
    let mut host = Host::new(program, hooks, runtime, renderer, inputs, &args);
    eprintln!("[remote] listening on {}", args.control.display());
    let code = host.run();
    host.close();
    inflight.drain(DRAIN_TIMEOUT);
    channel::cleanup(&args.control);
    // Exit rather than return: dropping an executor may block until its
    // blocking tasks finish, and nothing is left to wait for.
    std::process::exit(code)
}

struct Host<A: Program> {
    program: A,
    state: A::State,
    hooks: Hooks<A::Message>,
    runtime: HostRuntime<A>,
    /// One offscreen renderer draws every window.
    renderer: A::Renderer,
    inputs: mpsc::Receiver<Input<A::Message>>,
    /// The open pretend windows, oldest first.
    windows: Vec<WindowState>,
    /// A daemon: it opens its windows itself and outlives them.
    daemon: bool,
    /// `--size`, until the first window takes it.
    first_size: Option<Size>,
    /// The scale factor the pretend OS reports for a new window.
    os_scale: f32,
    appearance: Mode,
    clipboard: MemoryClipboard,
    /// When the last host event that was not a heartbeat arrived.
    last_activity: Instant,
    /// When `update` last ran, heartbeat or not.
    last_update: Instant,
    idle_waiters: Vec<IdleWaiter>,
    target_waiters: Vec<TargetWaiter>,
    last_target_check: Instant,
    /// Commands that arrived while an input command was settling.
    queued: VecDeque<(Request, mpsc::Sender<String>)>,
    /// `quit` is answered once the loop has actually ended.
    quit_replies: Vec<mpsc::Sender<String>>,
    /// When a `quit` the app has not acted on ends the process anyway.
    quit_deadline: Option<Instant>,
    quit_forced: bool,
    exit: Option<i32>,
    recording: Option<Recording>,
}

/// One pretend window.
struct WindowState {
    id: window::Id,
    settings: window::Settings,
    /// OS-logical size.
    size: Size,
    /// The scale factor the pretend OS reports.
    os_scale: f32,
    cache: user_interface::Cache,
    cursor: mouse::Cursor,
    /// Whether the left button is down, for the pointer a recording draws.
    pressed: bool,
    /// Whether the app maximized it; only the app's own query sees it.
    maximized: bool,
    /// When the interface wants its next `RedrawRequested`.
    redraw: window::RedrawRequest,
    last_frame: Instant,
}

impl WindowState {
    fn new(id: window::Id, settings: window::Settings, size: Size, os_scale: f32) -> Self {
        let size = clamp(&settings, size);
        Self {
            id,
            settings,
            size,
            os_scale,
            cache: user_interface::Cache::default(),
            cursor: mouse::Cursor::Unavailable,
            pressed: false,
            maximized: false,
            redraw: window::RedrawRequest::NextFrame,
            last_frame: Instant::now(),
        }
    }

    /// Whether its `RedrawRequested` is due at `now`.
    fn frame_due(&self, now: Instant) -> bool {
        match self.redraw {
            window::RedrawRequest::NextFrame => now >= self.last_frame + FRAME,
            window::RedrawRequest::At(at) => at <= now,
            window::RedrawRequest::Wait => false,
        }
    }
}

/// `size` limited to the window's `min_size` and `max_size`.
fn clamp(settings: &window::Settings, size: Size) -> Size {
    let mut size = size;
    if let Some(min) = settings.min_size {
        size = Size::new(size.width.max(min.width), size.height.max(min.height));
    }
    if let Some(max) = settings.max_size {
        size = Size::new(size.width.min(max.width), size.height.min(max.height));
    }
    size
}

struct IdleWaiter {
    quiet: Duration,
    since: Instant,
    reply: mpsc::Sender<String>,
}

/// A `wait-for` (`appear`) or `wait-gone`.
struct TargetWaiter {
    window: window::Id,
    target: Target,
    appear: bool,
    deadline: Instant,
    reply: mpsc::Sender<String>,
}

/// A running `record`: frames go through a bounded channel to a thread that
/// writes them to ffmpeg's stdin, so a slow encoder holds the loop back
/// instead of growing memory.
struct Recording {
    window: window::Id,
    path: PathBuf,
    started: Instant,
    /// Frames sent so far; frame `n` shows the time `started + n / fps`.
    written: u64,
    frames: mpsc::SyncSender<Arc<Vec<u8>>>,
    writer: std::thread::JoinHandle<Result<(), String>>,
    child: Child,
}

impl Recording {
    /// When the next frame is due.
    fn next_frame(&self) -> Instant {
        self.started + Duration::from_secs_f64(self.written as f64 / f64::from(RECORD_FPS))
    }

    /// Closes the stream and waits for ffmpeg to write the file.
    fn finish(self) -> String {
        let Recording {
            path,
            written,
            frames,
            writer,
            mut child,
            ..
        } = self;
        // Closing the channel ends the writer, which drops ffmpeg's stdin.
        drop(frames);
        let wrote = writer
            .join()
            .unwrap_or_else(|_| Err("writer panicked".to_owned()));
        // Read before waiting: a child blocked on a full stderr pipe never
        // exits.
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        let failure = match (wrote, child.wait()) {
            (Err(e), _) => Some(e),
            (Ok(()), Err(e)) => Some(e.to_string()),
            (Ok(()), Ok(status)) if !status.success() => Some(status.to_string()),
            (Ok(()), Ok(_)) => None,
        };
        match failure {
            Some(e) => format!("err ffmpeg: {e}: {}", stderr.trim()),
            None => format!(
                "ok {} {written} frames {:.1}s",
                path.display(),
                written as f64 / f64::from(RECORD_FPS)
            ),
        }
    }
}

impl<A> Host<A>
where
    A: Program + 'static,
{
    fn new(
        program: A,
        hooks: Hooks<A::Message>,
        runtime: HostRuntime<A>,
        renderer: A::Renderer,
        inputs: mpsc::Receiver<Input<A::Message>>,
        args: &HostArgs,
    ) -> Self {
        let (state, boot) = runtime.enter(|| program.boot());
        let main = program.window();
        let now = Instant::now();
        let mut host = Self {
            program,
            state,
            hooks,
            runtime,
            renderer,
            inputs,
            windows: Vec::new(),
            daemon: main.is_none(),
            first_size: args.size,
            os_scale: args.scale,
            appearance: args.appearance,
            clipboard: MemoryClipboard {
                standard: args.clipboard.clone(),
                primary: None,
            },
            last_activity: now,
            last_update: now,
            idle_waiters: Vec::new(),
            target_waiters: Vec::new(),
            last_target_check: now,
            queued: VecDeque::new(),
            quit_replies: Vec::new(),
            quit_deadline: None,
            quit_forced: false,
            exit: None,
            recording: None,
        };
        // An application's window exists before it boots; a daemon opens its
        // own through `window::open`.
        let main = main.map(|settings| {
            let size = host.first_size.take().unwrap_or(settings.size);
            let id = window::Id::unique();
            host.windows
                .push(WindowState::new(id, settings, size, host.os_scale));
            id
        });
        // Subscribed before the first event, so `listen_with` hears the size.
        host.resubscribe();
        host.spawn(boot);
        if let Some(id) = main {
            let size = host.ui_size(id);
            host.deliver(
                id,
                vec![Event::Window(window::Event::Opened {
                    position: None,
                    size,
                })],
            );
        }
        host
    }

    fn win(&self, id: window::Id) -> Option<&WindowState> {
        self.windows.iter().find(|w| w.id == id)
    }

    fn win_mut(&mut self, id: window::Id) -> Option<&mut WindowState> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// The window a command addresses: its 1-based number, the oldest
    /// without one.
    fn resolve(&self, number: Option<usize>) -> Result<window::Id, String> {
        if self.windows.is_empty() {
            return Err("no window open".to_owned());
        }
        let n = number.unwrap_or(1);
        n.checked_sub(1)
            .and_then(|i| self.windows.get(i))
            .map(|w| w.id)
            .ok_or_else(|| {
                format!(
                    "no window {n} (open: 1..{}, see windows)",
                    self.windows.len()
                )
            })
    }

    /// The OS scale times the app's own scale factor.
    fn total_scale(&self, id: window::Id) -> f32 {
        let os = self.win(id).map_or(self.os_scale, |w| w.os_scale);
        os * self.program.scale_factor(&self.state, id)
    }

    /// The frame size `render` produces.
    fn physical_size(&self, id: window::Id) -> Size<u32> {
        let (size, os) = self
            .win(id)
            .map_or((Size::new(1.0, 1.0), self.os_scale), |w| {
                (w.size, w.os_scale)
            });
        Size::new(
            (size.width * os).round().max(1.0) as u32,
            (size.height * os).round().max(1.0) as u32,
        )
    }

    /// The logical size the interface is laid out in.
    fn ui_size(&self, id: window::Id) -> Size {
        let physical = self.physical_size(id);
        let scale = self.total_scale(id);
        Size::new(
            physical.width as f32 / scale,
            physical.height as f32 / scale,
        )
    }

    fn theme(&self, id: window::Id) -> A::Theme {
        self.program
            .theme(&self.state, id)
            .unwrap_or_else(|| <A::Theme as Base>::default(self.appearance))
    }

    /// Builds the window's interface from the current state, hands it to
    /// `f`, and keeps its cache for the next build; `None` once the window
    /// is closed.
    fn with_ui<R>(
        &mut self,
        id: window::Id,
        f: impl FnOnce(
            &mut UserInterface<'_, A::Message, A::Theme, A::Renderer>,
            &mut A::Renderer,
            &mut MemoryClipboard,
        ) -> R,
    ) -> Option<R> {
        let size = self.ui_size(id);
        let cache = std::mem::take(&mut self.win_mut(id)?.cache);
        let mut ui = UserInterface::build(
            self.program.view(&self.state, id),
            size,
            cache,
            &mut self.renderer,
        );
        let result = f(&mut ui, &mut self.renderer, &mut self.clipboard);
        let cache = ui.into_cache();
        if let Some(window) = self.win_mut(id) {
            window.cache = cache;
        }
        Some(result)
    }

    /// Serves inputs until the app exits; returns the exit code.
    fn run(&mut self) -> i32 {
        loop {
            let input = match self.queued.pop_front() {
                Some((request, reply)) => Some(Input::Control(request, reply)),
                None => match self.wakeup() {
                    Some(after) => match self.inputs.recv_timeout(after) {
                        Ok(input) => Some(input),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return 1,
                    },
                    None => match self.inputs.recv() {
                        Ok(input) => Some(input),
                        Err(mpsc::RecvError) => return 1,
                    },
                },
            };
            match input {
                Some(Input::Host(event)) => self.host_event(event),
                Some(Input::Control(request, reply)) => self.control(request, reply),
                None => {}
            }
            if self.exit.is_none()
                && self
                    .quit_deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
            {
                self.quit_forced = true;
                self.exit = Some(0);
            }
            if let Some(code) = self.exit {
                return code;
            }
            self.frame();
            self.capture();
            self.answer_idle_waiters();
            self.answer_target_waiters();
        }
    }

    /// Answers everything still waiting once the loop has ended.
    fn close(&mut self) {
        let bye = if self.quit_forced { "ok forced" } else { "ok" };
        for reply in self.quit_replies.drain(..) {
            let _ = reply.send(bye.to_owned());
        }
        for waiter in self.idle_waiters.drain(..) {
            let _ = waiter.reply.send("err exiting".to_owned());
        }
        for waiter in self.target_waiters.drain(..) {
            let _ = waiter.reply.send("err exiting".to_owned());
        }
        for (_, reply) in self.queued.drain(..) {
            let _ = reply.send("err exiting".to_owned());
        }
        if let Some(recording) = self.recording.take() {
            let path = recording.path.clone();
            eprintln!(
                "[remote] recording {}: {}",
                path.display(),
                recording.finish()
            );
        }
        // Dropping the receiver makes every later command fail at once
        // instead of waiting for a loop that is gone.
        let (_, closed) = mpsc::channel();
        let inputs = std::mem::replace(&mut self.inputs, closed);
        for input in inputs.try_iter() {
            if let Input::Control(_, reply) = input {
                let _ = reply.send("err exiting".to_owned());
            }
        }
    }

    fn host_event(&mut self, event: HostEvent<A::Message>) {
        match event {
            HostEvent::Message(message) => {
                if !(self.hooks.heartbeat)(&message) {
                    self.last_activity = Instant::now();
                }
                self.update(message);
            }
            HostEvent::Action(action) => {
                self.last_activity = Instant::now();
                self.perform(action);
            }
        }
    }

    /// Runs the app's `update`, starts the task it returns, resubscribes,
    /// and asks for a frame the way a window redraws after an update.
    fn update(&mut self, message: A::Message) {
        let task = self
            .runtime
            .enter(|| self.program.update(&mut self.state, message));
        self.spawn(task);
        self.resubscribe();
        for window in &mut self.windows {
            window.redraw = window::RedrawRequest::NextFrame;
        }
        self.last_update = Instant::now();
    }

    fn spawn(&mut self, task: Task<A::Message>) {
        if let Some(stream) = task::into_stream(task) {
            self.runtime.run(stream.map(HostEvent::Action).boxed());
        }
    }

    fn resubscribe(&mut self) {
        let recipes = subscription::into_recipes(self.runtime.enter(|| {
            self.program
                .subscription(&self.state)
                .map(HostEvent::Message)
        }));
        self.runtime.track(recipes);
    }

    /// Feeds events to the window's interface, broadcasts them to the
    /// subscriptions with the status the interface gave each, then applies
    /// the messages the widgets published.
    fn deliver(&mut self, id: window::Id, events: Vec<Event>) {
        let Some(window) = self.win_mut(id) else {
            return;
        };
        let mut is_frame = false;
        for event in &events {
            match event {
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    window.cursor = mouse::Cursor::Available(*position);
                }
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    window.pressed = true;
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    window.pressed = false;
                }
                Event::Window(window::Event::RedrawRequested(_)) => is_frame = true,
                _ => {}
            }
        }
        let cursor = window.cursor;
        let mut messages = Vec::new();
        let Some((state, statuses)) = self.with_ui(id, |ui, renderer, clipboard| {
            ui.update(&events, cursor, renderer, clipboard, &mut messages)
        }) else {
            return;
        };

        let wanted = match state {
            user_interface::State::Updated { redraw_request, .. } => redraw_request,
            user_interface::State::Outdated => window::RedrawRequest::NextFrame,
        };
        // A frame answers the request that was pending; any other event can
        // only bring the next frame closer.
        if let Some(window) = self.win_mut(id) {
            window.redraw = if is_frame {
                wanted
            } else {
                window.redraw.min(wanted)
            };
        }

        for (event, status) in events.into_iter().zip(statuses) {
            self.runtime.broadcast(subscription::Event::Interaction {
                window: id,
                event,
                status,
            });
        }
        for message in messages {
            self.update(message);
        }
    }

    /// Delivers `RedrawRequested` to every window whose interface asked for it.
    fn frame(&mut self) {
        let now = Instant::now();
        let due: Vec<window::Id> = self
            .windows
            .iter()
            .filter(|w| w.frame_due(now))
            .map(|w| w.id)
            .collect();
        for id in due {
            if let Some(window) = self.win_mut(id) {
                window.last_frame = now;
            }
            self.deliver(id, vec![Event::Window(window::Event::RedrawRequested(now))]);
        }
    }

    fn redraw_due(&self, now: Instant) -> bool {
        self.windows.iter().any(|w| match w.redraw {
            window::RedrawRequest::NextFrame => true,
            window::RedrawRequest::At(at) => at <= now,
            window::RedrawRequest::Wait => false,
        })
    }

    /// How long the loop may sleep before a frame, a recorded frame, a
    /// waiter or the `quit` deadline is due. `None` sleeps until the next
    /// input.
    fn wakeup(&self) -> Option<Duration> {
        let now = Instant::now();
        let frame = self
            .windows
            .iter()
            .filter_map(|w| match w.redraw {
                window::RedrawRequest::NextFrame => Some(w.last_frame + FRAME),
                window::RedrawRequest::At(at) => Some(at),
                window::RedrawRequest::Wait => None,
            })
            .min();
        let idle = self.idle_waiters.iter().flat_map(|w| {
            let quiet_at = self.last_activity.max(w.since) + w.quiet;
            let give_up = w.since + IDLE_TIMEOUT;
            [
                (quiet_at > now).then_some(quiet_at),
                (self.last_update + UPDATE_GAP > now).then_some(self.last_update + UPDATE_GAP),
                Some(quiet_at + ANIMATING_AFTER),
                Some(give_up),
            ]
            .into_iter()
            .flatten()
        });
        let check =
            (!self.target_waiters.is_empty()).then_some(self.last_target_check + TARGET_POLL);
        let deadlines = self.target_waiters.iter().map(|w| w.deadline);
        let capture = self.recording.as_ref().map(Recording::next_frame);
        frame
            .into_iter()
            .chain(capture)
            .chain(idle)
            .chain(check)
            .chain(deadlines)
            .chain(self.quit_deadline)
            .min()
            .map(|at| at.saturating_duration_since(now))
    }

    /// `ok idle` once nothing but heartbeats happened for the quiet time and
    /// no frame is pending; `ok animating` when only redraws kept it busy for
    /// another second; `err timeout` after ten seconds of real activity.
    fn answer_idle_waiters(&mut self) {
        if self.idle_waiters.is_empty() {
            return;
        }
        let now = Instant::now();
        let redraw_due = self.redraw_due(now) || now < self.last_update + UPDATE_GAP;
        let last_activity = self.last_activity;
        self.idle_waiters.retain(|waiter| {
            let quiet_at = last_activity.max(waiter.since) + waiter.quiet;
            let answer = if now >= quiet_at && !redraw_due {
                "ok idle"
            } else if now >= quiet_at + ANIMATING_AFTER {
                "ok animating"
            } else if now >= waiter.since + IDLE_TIMEOUT {
                "err timeout"
            } else {
                return true;
            };
            let _ = waiter.reply.send(answer.to_owned());
            false
        });
    }

    fn answer_target_waiters(&mut self) {
        if self.target_waiters.is_empty() {
            return;
        }
        let now = Instant::now();
        let expired = self.target_waiters.iter().any(|w| now >= w.deadline);
        if now < self.last_target_check + TARGET_POLL && !expired {
            return;
        }
        self.last_target_check = now;
        let mut inventories: Vec<(window::Id, Vec<Node>)> = Vec::new();
        for i in 0..self.target_waiters.len() {
            let id = self.target_waiters[i].window;
            if self.win(id).is_some() && !inventories.iter().any(|(seen, _)| *seen == id) {
                let nodes = self.inventory(id);
                inventories.push((id, nodes));
            }
        }
        self.target_waiters.retain(|waiter| {
            let nodes = inventories
                .iter()
                .find(|(id, _)| *id == waiter.window)
                .map(|(_, nodes)| nodes);
            let answer = match nodes.map(|nodes| target_state(nodes, &waiter.target, waiter.appear))
            {
                None => window_closed(waiter.appear).to_owned(),
                Some(Some(answer)) => answer,
                Some(None) if now >= waiter.deadline => "err timeout".to_owned(),
                Some(None) => return true,
            };
            let _ = waiter.reply.send(answer);
            false
        });
    }

    fn control(&mut self, request: Request, reply: mpsc::Sender<String>) {
        let Request { window, command } = request;
        // Commands about the process, not a window, ignore the selector.
        let answer = match command {
            Command::Help => self.help(),
            Command::Windows => self.windows(),
            Command::Info => self.info(window),
            Command::WaitIdle(quiet) => {
                self.idle_waiters.push(IdleWaiter {
                    quiet,
                    since: Instant::now(),
                    reply,
                });
                return;
            }
            Command::RecordStop => match self.recording.take() {
                Some(recording) => recording.finish(),
                None => "err not recording".to_owned(),
            },
            Command::Appearance(mode) => {
                self.appearance = mode;
                self.runtime
                    .broadcast(subscription::Event::SystemThemeChanged(mode));
                for window in &mut self.windows {
                    window.redraw = window::RedrawRequest::NextFrame;
                }
                self.settle();
                "ok".to_owned()
            }
            Command::Clip => match self.clipboard.read(clipboard::Kind::Standard) {
                Some(text) => format!("ok {text}"),
                None => "ok".to_owned(),
            },
            Command::ClipSet(text) => {
                self.clipboard.write(clipboard::Kind::Standard, text);
                "ok".to_owned()
            }
            Command::Quit => {
                self.quit_replies.push(reply);
                self.quit();
                return;
            }
            Command::Custom { name, args } => self.custom(&name, &args),
            command => match self.resolve(window) {
                Ok(id) => return self.window_command(id, command, reply),
                Err(e) => format!("err {e}"),
            },
        };
        let _ = reply.send(answer);
    }

    /// `ok N`, then `INDEX WxH scale S title="TITLE"` for each open window.
    fn windows(&self) -> String {
        let mut out = format!("ok {}", self.windows.len());
        for (i, window) in self.windows.iter().enumerate() {
            let size = self.ui_size(window.id);
            let title = self.program.title(&self.state, window.id);
            out.push_str(&format!(
                "\n{} {}x{} scale {} title=\"{}\"",
                i + 1,
                size.width,
                size.height,
                self.total_scale(window.id),
                title.replace('"', "\\\"")
            ));
        }
        out
    }

    fn info(&self, window: Option<usize>) -> String {
        let id = match self.resolve(window) {
            Ok(id) => Some(id),
            // A daemon may have no window yet.
            Err(_) if window.is_none() => None,
            Err(e) => return format!("err {e}"),
        };
        let size = id.map_or_else(String::new, |id| {
            let size = self.ui_size(id);
            format!(
                " size {}x{} scale {}",
                size.width,
                size.height,
                self.total_scale(id)
            )
        });
        format!(
            "ok pid {} app {}{size} appearance {} backend {} windows {}",
            std::process::id(),
            A::name(),
            mode_name(self.appearance),
            self.renderer.name(),
            self.windows.len()
        )
    }

    /// Exits at once unless a window handles close requests itself; then
    /// the oldest such window hears one and the app decides, as for its
    /// close button, until the deadline ends it.
    fn quit(&mut self) {
        let asks = self
            .windows
            .iter()
            .find(|w| !w.settings.exit_on_close_request)
            .map(|w| w.id);
        match asks {
            None => self.exit = Some(0),
            Some(id) if self.quit_deadline.is_none() => {
                self.quit_deadline = Some(Instant::now() + QUIT_GRACE);
                self.deliver(id, vec![Event::Window(window::Event::CloseRequested)]);
            }
            Some(_) => {}
        }
    }

    /// A command that acts on the window `id`.
    fn window_command(&mut self, id: window::Id, command: Command, reply: mpsc::Sender<String>) {
        let answer = match command {
            Command::Size => {
                let size = self.ui_size(id);
                format!("ok {} {} {}", size.width, size.height, self.total_scale(id))
            }
            Command::Move { to, over } if over.is_zero() => {
                self.pointer(id, vec![moved(to)], Duration::ZERO)
            }
            Command::Move { to, over } => {
                let from = self.win(id).and_then(|w| w.cursor.position()).unwrap_or(to);
                let steps = u32::try_from(over.as_millis() / 16)
                    .unwrap_or(u32::MAX)
                    .max(1);
                self.pointer(id, path(from, to, steps).collect(), over / steps)
            }
            Command::Down(button) => self.pointer(id, vec![pressed(button)], Duration::ZERO),
            Command::Up(button) => self.pointer(id, vec![released(button)], Duration::ZERO),
            Command::Click(at, button, modifiers) => self.click(id, at, button, modifiers),
            Command::DoubleClick(at) => self.pointer(
                id,
                vec![
                    moved(at),
                    pressed(mouse::Button::Left),
                    released(mouse::Button::Left),
                    pressed(mouse::Button::Left),
                    released(mouse::Button::Left),
                ],
                Duration::ZERO,
            ),
            Command::Drag {
                from,
                to,
                steps,
                over,
            } => {
                let events: Vec<Event> = [moved(from), pressed(mouse::Button::Left)]
                    .into_iter()
                    .chain(path(from, to, steps))
                    .chain([released(mouse::Button::Left)])
                    .collect();
                self.pointer(id, events, over / steps)
            }
            Command::Scroll { at, dx, dy } => self.pointer(
                id,
                vec![
                    moved(at),
                    Event::Mouse(mouse::Event::WheelScrolled {
                        delta: mouse::ScrollDelta::Lines { x: dx, y: dy },
                    }),
                ],
                Duration::ZERO,
            ),
            Command::Key(keystroke) => self.input(id, key_events(&keystroke)),
            Command::KeyDown(keystroke) => self.input(id, key_down_events(&keystroke)),
            Command::KeyUp(keystroke) => self.input(id, key_up_events(&keystroke)),
            Command::Type(text) => {
                let events: Vec<Event> = text
                    .chars()
                    .flat_map(|c| key_events(&Keystroke::character(c, Modifiers::empty())))
                    .collect();
                self.input(id, events)
            }
            Command::Tap { target, nth } => {
                let nodes = self.inventory(id);
                match target::pick(&nodes, &target, nth) {
                    Ok(node) => {
                        let (x, y) = target::centre(node.rect());
                        let _ = self.click(
                            id,
                            Point::new(x, y),
                            mouse::Button::Left,
                            Modifiers::empty(),
                        );
                        format!("ok {x} {y}")
                    }
                    Err(e) => format!("err {e}"),
                }
            }
            Command::Find { target, nth } => {
                let nodes = self.inventory(id);
                match target::pick(&nodes, &target, nth) {
                    Ok(node) => format!("ok {}", target::rect_words(node.rect())),
                    Err(e) => format!("err {e}"),
                }
            }
            Command::FindAll(target) => {
                let nodes = self.inventory(id);
                let lines: Vec<String> = nodes
                    .iter()
                    .filter(|n| target.matches(n))
                    .map(|n| {
                        let words = target::rect_words(n.rect());
                        if n.visible.is_some() {
                            words
                        } else {
                            format!("{words} hidden")
                        }
                    })
                    .collect();
                if lines.is_empty() {
                    format!("err {}", target::not_found(&nodes, &target))
                } else {
                    format!("ok {}\n{}", lines.len(), lines.join("\n"))
                }
            }
            Command::Focused => self.focused(id),
            Command::Tree { all } => {
                let nodes = self.inventory(id);
                let shown = target::filtered(&nodes, all);
                let mut out = format!("ok {}", shown.len());
                for (i, node) in shown.iter().enumerate() {
                    out.push('\n');
                    out.push_str(&target::node_line(i + 1, node));
                }
                out
            }
            Command::WaitFor { target, timeout } => {
                return self.wait_target(id, target, true, timeout, reply);
            }
            Command::WaitGone { target, timeout } => {
                return self.wait_target(id, target, false, timeout, reply);
            }
            Command::Screenshot(shot) => self.screenshot_command(id, shot),
            Command::Record(path) => self.record(id, path),
            // A video has one frame size.
            Command::Resize(_) | Command::Scale(_)
                if self.recording.as_ref().is_some_and(|r| r.window == id) =>
            {
                "err stop the recording first".to_owned()
            }
            Command::Resize(size) => {
                if let Some(window) = self.win_mut(id) {
                    window.size = clamp(&window.settings, size);
                }
                let size = self.ui_size(id);
                let _ = self.input(id, [Event::Window(window::Event::Resized(size))]);
                format!("ok {} {}", size.width, size.height)
            }
            Command::Scale(scale) => {
                if let Some(window) = self.win_mut(id) {
                    window.os_scale = scale;
                }
                let size = self.ui_size(id);
                self.input(
                    id,
                    [
                        Event::Window(window::Event::Rescaled(scale)),
                        Event::Window(window::Event::Resized(size)),
                    ],
                )
            }
            Command::Drop(path) => self.input(
                id,
                [
                    Event::Window(window::Event::FileHovered(path.clone())),
                    Event::Window(window::Event::FileDropped(path)),
                ],
            ),
            Command::Help
            | Command::Info
            | Command::Windows
            | Command::WaitIdle(_)
            | Command::RecordStop
            | Command::Appearance(_)
            | Command::Clip
            | Command::ClipSet(_)
            | Command::Quit
            | Command::Custom { .. } => unreachable!("answered by control"),
        };
        let _ = reply.send(answer);
    }
}

impl<A> Host<A>
where
    A: Program + 'static,
{
    fn help(&self) -> String {
        let mut lines = vec!["ok".to_owned()];
        lines.extend(protocol::help_lines());
        lines.extend(protocol::help_footer());
        lines.extend(
            self.hooks
                .commands
                .iter()
                .map(|c| format!("custom: {} - {}", c.name, c.summary)),
        );
        lines.join("\n")
    }

    /// Runs a registered command's message through `update`.
    fn custom(&mut self, name: &str, args: &str) -> String {
        let built = self
            .hooks
            .commands
            .iter()
            .find(|c| c.name == name)
            .map(|c| (c.build)(args));
        match built {
            None => format!("err unknown command \"{name}\" (see help)"),
            Some(Err(e)) => format!("err {name}: {e}"),
            Some(Ok(message)) => {
                self.last_activity = Instant::now();
                self.update(message);
                self.settle();
                "ok".to_owned()
            }
        }
    }

    /// Answers at once when the target is already there (or gone), and
    /// leaves a waiter for the loop otherwise.
    fn wait_target(
        &mut self,
        id: window::Id,
        target: Target,
        appear: bool,
        timeout: Duration,
        reply: mpsc::Sender<String>,
    ) {
        let nodes = self.inventory(id);
        if let Some(answer) = target_state(&nodes, &target, appear) {
            let _ = reply.send(answer);
            return;
        }
        self.last_target_check = Instant::now();
        self.target_waiters.push(TargetWaiter {
            window: id,
            target,
            appear,
            deadline: Instant::now() + timeout,
            reply,
        });
    }

    fn click(
        &mut self,
        id: window::Id,
        at: Point,
        button: mouse::Button,
        modifiers: Modifiers,
    ) -> String {
        let mut events = vec![moved(at), pressed(button), released(button)];
        if !modifiers.is_empty() {
            events.insert(
                0,
                Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)),
            );
            events.push(Event::Keyboard(iced::keyboard::Event::ModifiersChanged(
                Modifiers::empty(),
            )));
        }
        self.pointer(id, events, Duration::ZERO)
    }

    /// Delivers input one event at a time, as a window would across frames,
    /// then lets the answers from subscriptions arrive before replying.
    fn input(&mut self, id: window::Id, events: impl IntoIterator<Item = Event>) -> String {
        for event in events {
            self.deliver(id, vec![event]);
            if self.exit.is_some() {
                break;
            }
        }
        self.settle();
        "ok".to_owned()
    }

    /// Delivers pointer events with the loop running in between, the way a
    /// hand moves a mouse: a drag starts through a subscription message, so
    /// the press must be processed before the first move arrives. A zero
    /// `interval` settles after each event; otherwise event `i` is followed
    /// by running the loop until `start + interval * (i + 1)`.
    fn pointer(&mut self, id: window::Id, events: Vec<Event>, interval: Duration) -> String {
        let start = Instant::now();
        for (i, event) in (1u32..).zip(events) {
            self.deliver(id, vec![event]);
            if self.exit.is_some() {
                break;
            }
            if interval.is_zero() {
                self.settle();
            } else {
                self.pump(start + interval * i);
            }
        }
        self.settle();
        "ok".to_owned()
    }

    fn settle(&mut self) {
        let start = Instant::now();
        while self.exit.is_none() && start.elapsed() < SETTLE_MAX {
            match self.inputs.recv_timeout(SETTLE) {
                Ok(Input::Host(event)) => self.host_event(event),
                Ok(Input::Control(request, reply)) => self.queued.push_back((request, reply)),
                Err(_) => break,
            }
            self.frame();
            self.capture();
        }
    }

    /// Runs the loop until `until`: host events, frames and recorded
    /// frames, with control commands queued for later.
    fn pump(&mut self, until: Instant) {
        while self.exit.is_none() {
            let now = Instant::now();
            if now >= until {
                break;
            }
            let wait = self
                .wakeup()
                .map_or(until - now, |after| after.min(until - now));
            match self.inputs.recv_timeout(wait) {
                Ok(Input::Host(event)) => self.host_event(event),
                Ok(Input::Control(request, reply)) => self.queued.push_back((request, reply)),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            self.frame();
            self.capture();
        }
    }

    /// Every widget the window's interface reports, in tree order, double
    /// reports merged.
    fn inventory(&mut self, id: window::Id) -> Vec<Node> {
        let nodes = self
            .with_ui(id, |ui, renderer, _| {
                let mut selector = target::select.find_all();
                ui.operate(renderer, &mut operation::black_box(&mut selector));
                match selector.finish() {
                    Outcome::Some(nodes) => nodes,
                    _ => Vec::new(),
                }
            })
            .unwrap_or_default();
        target::dedupe(nodes)
    }

    /// `ok X Y W H` of the widget that holds the keyboard focus, `err none` without one.
    fn focused(&mut self, id: window::Id) -> String {
        let found = self
            .with_ui(id, |ui, renderer, _| {
                let mut selector = iced_selector::is_focused().find();
                ui.operate(renderer, &mut operation::black_box(&mut selector));
                match selector.finish() {
                    Outcome::Some(found) => found,
                    _ => None,
                }
            })
            .flatten();
        match found {
            Some(found) => match found.visible_bounds() {
                Some(b) => format!("ok {}", target::rect_words(b)),
                None => format!("err not visible: {}", target::rect_words(found.bounds())),
            },
            None => "err none".to_owned(),
        }
    }

    /// Render, annotate, crop, zoom, write.
    fn screenshot_command(&mut self, id: window::Id, shot: Shot) -> String {
        let scale = self.total_scale(id);
        let Some(mut frame) = self.render(id) else {
            return "err window closed".to_owned();
        };
        let mut lines = Vec::new();
        if shot.annotate {
            let nodes = self.inventory(id);
            let shown = target::filtered(&nodes, false);
            let mut boxes = Vec::new();
            for (i, node) in shown.iter().enumerate() {
                if let Some(visible) = node.visible {
                    boxes.push((i + 1, visible, node.kind.color()));
                    lines.push(target::node_line(i + 1, node));
                }
            }
            image::annotate(&mut frame, &boxes, scale);
        }
        if let Some(rect) = shot.crop {
            frame = match image::crop(&frame, rect, scale) {
                Ok(frame) => frame,
                Err(e) => return format!("err {e}"),
            };
        }
        let frame = image::zoom(&frame, shot.zoom);
        if let Err(e) = image::write_png(&shot.path, &frame) {
            return format!("err {}: {e}", shot.path.display());
        }
        let mut out = format!(
            "ok {} {}x{} scale {scale}",
            shot.path.display(),
            frame.size.width,
            frame.size.height
        );
        for line in lines {
            out.push('\n');
            out.push_str(&line);
        }
        out
    }

    /// Draws a frame of the window with its cursor and reads it back as
    /// RGBA; `None` once the window is closed.
    fn render(&mut self, id: window::Id) -> Option<Frame> {
        let theme = self.theme(id);
        let style = self.program.style(&self.state, &theme);
        let cursor = self.win(id)?.cursor;
        let mut messages = Vec::new();
        self.with_ui(id, |ui, renderer, clipboard| {
            let _ = ui.update(
                &[Event::Window(
                    window::Event::RedrawRequested(Instant::now()),
                )],
                cursor,
                renderer,
                clipboard,
                &mut messages,
            );
            ui.draw(
                renderer,
                &theme,
                &renderer::Style {
                    text_color: style.text_color,
                },
                cursor,
            );
        })?;
        let physical = self.physical_size(id);
        let scale = self.total_scale(id);
        let rgba =
            Headless::screenshot(&mut self.renderer, physical, scale, style.background_color);
        for message in messages {
            self.update(message);
        }
        Some(Frame {
            rgba,
            size: physical,
        })
    }

    /// Starts ffmpeg on a raw RGBA stream of the frames `capture` renders.
    fn record(&mut self, id: window::Id, path: PathBuf) -> String {
        if let Some(recording) = &self.recording {
            return format!("err already recording {}", recording.path.display());
        }
        // ffmpeg opens the output only once it has read the first frame, so
        // a missing directory would otherwise surface at `record-stop`.
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty())
            && !dir.is_dir()
        {
            return format!("err no directory {}", dir.display());
        }
        let size = self.physical_size(id);
        let spawned = std::process::Command::new("ffmpeg")
            .args(["-loglevel", "error", "-y", "-f", "rawvideo"])
            .args(["-pixel_format", "rgba", "-video_size"])
            .arg(format!("{}x{}", size.width, size.height))
            .arg("-framerate")
            .arg(RECORD_FPS.to_string())
            .args(["-i", "-", "-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2"])
            .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "20"])
            .args(["-pix_fmt", "yuv420p", "-movflags", "+faststart"])
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => return format!("err ffmpeg: {e}"),
        };
        let Some(mut stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return "err ffmpeg: no stdin".to_owned();
        };
        let (frames, received) = mpsc::sync_channel::<Arc<Vec<u8>>>(60);
        let writer = std::thread::spawn(move || {
            for frame in received {
                stdin.write_all(&frame).map_err(|e| e.to_string())?;
            }
            Ok(())
        });
        let reply = format!(
            "ok recording {} {}x{}",
            path.display(),
            size.width,
            size.height
        );
        self.recording = Some(Recording {
            window: id,
            path,
            started: Instant::now(),
            written: 0,
            frames,
            writer,
            child,
        });
        self.capture();
        reply
    }

    /// Sends the frame a recording is due, repeated for every slot a late
    /// capture missed, so the video keeps wall-clock time.
    fn capture(&mut self) {
        let now = Instant::now();
        let id = match &self.recording {
            Some(recording) if now >= recording.next_frame() => recording.window,
            _ => return,
        };
        let scale = self.total_scale(id);
        let Some(mut frame) = self.render(id) else {
            return;
        };
        if let Some(window) = self.win(id)
            && let Some(at) = window.cursor.position()
        {
            image::draw_pointer(&mut frame, at, scale, window.pressed);
        }
        let Some(recording) = &mut self.recording else {
            return;
        };
        let frame = Arc::new(frame.rgba);
        let slots =
            ((now - recording.started).as_secs_f64() * f64::from(RECORD_FPS)).floor() as u64 + 1;
        for _ in recording.written..slots {
            // A failed writer is reported by `finish`.
            let _ = recording.frames.send(frame.clone());
        }
        recording.written = recording.written.max(slots);
    }

    fn perform(&mut self, action: Action<A::Message>) {
        match action {
            Action::Output(message) => self.update(message),
            Action::LoadFont { bytes, channel } => {
                font_system().write().expect("font system").load_font(bytes);
                let _ = channel.send(Ok(()));
            }
            Action::Widget(operation) => self.operate(operation),
            Action::Clipboard(action) => match action {
                iced_runtime::clipboard::Action::Read { target, channel } => {
                    let _ = channel.send(self.clipboard.read(target));
                }
                iced_runtime::clipboard::Action::Write { target, contents } => {
                    self.clipboard.write(target, contents);
                }
            },
            Action::Window(action) => self.window_action(action),
            Action::System(action) => self.system_action(action),
            Action::Image(_) | Action::Reload => {}
            Action::Exit => self.exit = Some(0),
        }
    }

    /// The pretend desktop answers with the appearance it was started with
    /// or switched to.
    fn system_action(&self, action: iced_runtime::system::Action) {
        use iced_runtime::system::{Action as System, Information};
        match action {
            System::GetTheme(sender) => {
                let _ = sender.send(self.appearance);
            }
            System::GetInformation(sender) => {
                let _ = sender.send(Information {
                    system_name: Some("headless".to_owned()),
                    system_kernel: None,
                    system_version: None,
                    system_short_version: None,
                    cpu_brand: String::new(),
                    cpu_cores: None,
                    memory_total: 0,
                    memory_used: None,
                    graphics_backend: self.renderer.name(),
                    graphics_adapter: "headless".to_owned(),
                });
            }
            System::NotifyTheme(_) => {}
        }
    }

    /// Runs a widget operation on every window's interface, as a windowed
    /// run does.
    fn operate(&mut self, operation: Box<dyn Operation>) {
        let ids: Vec<window::Id> = self.windows.iter().map(|w| w.id).collect();
        let mut operation = Some(operation);
        while let Some(mut current) = operation.take() {
            for &id in &ids {
                let _ = self.with_ui(id, |ui, renderer, _| ui.operate(renderer, &mut current));
            }
            if let Outcome::Chain(next) = current.finish() {
                operation = Some(next);
            }
        }
        // A focus or scroll changes what is drawn.
        for window in &mut self.windows {
            window.redraw = window::RedrawRequest::NextFrame;
        }
    }

    fn window_action(&mut self, action: iced_runtime::window::Action) {
        use iced_runtime::window::Action as Window;
        // An unknown id drops the sender, as for a window that is gone.
        match action {
            Window::Open(id, settings, sender) => {
                let size = self.first_size.take().unwrap_or(settings.size);
                self.windows
                    .push(WindowState::new(id, settings, size, self.os_scale));
                let size = self.ui_size(id);
                self.deliver(
                    id,
                    vec![Event::Window(window::Event::Opened {
                        position: None,
                        size,
                    })],
                );
                let _ = sender.send(id);
            }
            Window::Close(id) => self.close_window(id),
            Window::GetOldest(sender) => {
                let _ = sender.send(self.windows.first().map(|w| w.id));
            }
            Window::GetLatest(sender) => {
                let _ = sender.send(self.windows.last().map(|w| w.id));
            }
            Window::GetSize(id, sender) => {
                if self.win(id).is_some() {
                    let _ = sender.send(self.ui_size(id));
                }
            }
            Window::GetMaximized(id, sender) => {
                if let Some(window) = self.win(id) {
                    let _ = sender.send(window.maximized);
                }
            }
            Window::Resize(id, size) => {
                let Some(window) = self.win_mut(id) else {
                    return;
                };
                window.size = clamp(&window.settings, size);
                let size = self.ui_size(id);
                self.deliver(id, vec![Event::Window(window::Event::Resized(size))]);
            }
            // A window manager resizes a window it maximizes; the app hears
            // the resize and asks for the new state.
            Window::Maximize(id, maximized) => {
                let Some(window) = self.win_mut(id) else {
                    return;
                };
                window.maximized = maximized;
                let size = self.ui_size(id);
                self.deliver(id, vec![Event::Window(window::Event::Resized(size))]);
            }
            Window::ToggleMaximize(id) => {
                let Some(window) = self.win_mut(id) else {
                    return;
                };
                window.maximized = !window.maximized;
                let size = self.ui_size(id);
                self.deliver(id, vec![Event::Window(window::Event::Resized(size))]);
            }
            Window::GetMinimized(_, sender) => {
                let _ = sender.send(None);
            }
            Window::GetPosition(id, sender) => {
                if self.win(id).is_some() {
                    let _ = sender.send(Some(Point::ORIGIN));
                }
            }
            Window::GetScaleFactor(id, sender) => {
                if let Some(window) = self.win(id) {
                    let _ = sender.send(window.os_scale);
                }
            }
            Window::GetMode(id, sender) => {
                if self.win(id).is_some() {
                    let _ = sender.send(window::Mode::Windowed);
                }
            }
            Window::Screenshot(id, sender) => {
                let scale = self.total_scale(id);
                if let Some(frame) = self.render(id) {
                    let _ = sender.send(window::Screenshot::new(frame.rgba, frame.size, scale));
                }
            }
            _ => eprintln!("[remote] ignored window action"),
        }
    }

    /// Removes the window, tells the subscriptions, and answers what waited
    /// on it. An application ends with its last window; a daemon only
    /// through `iced::exit`.
    fn close_window(&mut self, id: window::Id) {
        let Some(i) = self.windows.iter().position(|w| w.id == id) else {
            return;
        };
        self.windows.remove(i);
        self.runtime.broadcast(subscription::Event::Interaction {
            window: id,
            event: Event::Window(window::Event::Closed),
            status: event::Status::Ignored,
        });
        self.target_waiters.retain(|waiter| {
            if waiter.window != id {
                return true;
            }
            let _ = waiter.reply.send(window_closed(waiter.appear).to_owned());
            false
        });
        if self.recording.as_ref().is_some_and(|r| r.window == id)
            && let Some(recording) = self.recording.take()
        {
            let path = recording.path.clone();
            eprintln!(
                "[remote] recording {}: {}",
                path.display(),
                recording.finish()
            );
        }
        if !self.daemon && self.windows.is_empty() {
            self.exit = Some(0);
        }
    }
}

/// What a target waiter answers when its window closes: whatever it waited
/// to vanish is gone, whatever it waited for never comes.
fn window_closed(appear: bool) -> &'static str {
    if appear { "err window closed" } else { "ok" }
}

/// The reply a target waiter gives now, or `None` while it keeps waiting.
fn target_state(nodes: &[Node], target: &Target, appear: bool) -> Option<String> {
    let first: Option<Rectangle> = nodes
        .iter()
        .filter(|n| target.matches(n))
        .find_map(|n| n.visible);
    match (appear, first) {
        (true, Some(visible)) => Some(format!("ok {}", target::rect_words(visible))),
        (false, None) => Some("ok".to_owned()),
        _ => None,
    }
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Light => "light",
        Mode::Dark | Mode::None => "dark",
    }
}

fn moved(position: Point) -> Event {
    Event::Mouse(mouse::Event::CursorMoved { position })
}

fn pressed(button: mouse::Button) -> Event {
    Event::Mouse(mouse::Event::ButtonPressed(button))
}

fn released(button: mouse::Button) -> Event {
    Event::Mouse(mouse::Event::ButtonReleased(button))
}

/// `steps` evenly spaced moves from `from` (exclusive) to `to` (inclusive).
fn path(from: Point, to: Point, steps: u32) -> impl Iterator<Item = Event> {
    (1..=steps).map(move |i| {
        let t = i as f32 / steps as f32;
        moved(Point::new(
            from.x + (to.x - from.x) * t,
            from.y + (to.y - from.y) * t,
        ))
    })
}

/// The clipboard a window would share with the desktop, kept in memory so a
/// headless run never touches the user's.
#[derive(Default)]
struct MemoryClipboard {
    standard: Option<String>,
    primary: Option<String>,
}

impl Clipboard for MemoryClipboard {
    fn read(&self, kind: clipboard::Kind) -> Option<String> {
        match kind {
            clipboard::Kind::Standard => self.standard.clone(),
            clipboard::Kind::Primary => self.primary.clone(),
        }
    }

    fn write(&mut self, kind: clipboard::Kind, contents: String) {
        match kind {
            clipboard::Kind::Standard => self.standard = Some(contents),
            clipboard::Kind::Primary => self.primary = Some(contents),
        }
    }
}

/// Has the loop answer one command line.
fn answer<M>(line: &str, inputs: &mpsc::Sender<Input<M>>) -> String {
    match Request::parse(line) {
        Err(e) => format!("err {e}"),
        Ok(request) => {
            let (reply_tx, reply_rx) = mpsc::channel();
            if inputs.send(Input::Control(request, reply_tx)).is_err() {
                "err exiting".to_owned()
            } else {
                reply_rx.recv().unwrap_or_else(|_| "err exiting".to_owned())
            }
        }
    }
}
