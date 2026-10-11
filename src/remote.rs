//! The builder an app wraps its `iced::Application` or `iced::Daemon` in, and the argv
//! dispatch between the windowed app, the headless host and `ctl`.

use std::ffi::OsString;
use std::path::Path;

use iced::message::{MaybeClone, MaybeDebug};
use iced::{Application, Daemon, Program};

use crate::args::{self, HostArgs};
use crate::host::{self, Custom, Heartbeat, Hooks};
use crate::protocol;

/// An iced program that runs in its own windows: an `iced::Application` or
/// an `iced::Daemon`.
pub trait Runnable: Program + 'static {
    /// Runs the program in its windows, as its own `run` does.
    fn run(self) -> iced::Result;
}

impl<P: Program + 'static> Runnable for Application<P>
where
    P::Message: MaybeDebug + MaybeClone,
{
    fn run(self) -> iced::Result {
        Application::run(self)
    }
}

impl<P: Program + 'static> Runnable for Daemon<P>
where
    P::Message: MaybeDebug + MaybeClone,
{
    fn run(self) -> iced::Result {
        Daemon::run(self)
    }
}

/// An app that can also run headless under remote control.
///
/// `Remote::run` looks at the command line: `<bin> ctl ...` runs the
/// control client, `--headless --control <socket>` runs the app offscreen
/// behind a control socket, anything else runs the app in its window.
pub struct Remote<A: Runnable> {
    app: A,
    heartbeat: Heartbeat<A::Message>,
    commands: Vec<Custom<A::Message>>,
    windowed: Option<Box<dyn FnOnce()>>,
    headless: Option<Box<dyn FnOnce()>>,
}

impl<A> Remote<A>
where
    A: Runnable,
    A::Message: MaybeDebug + MaybeClone,
{
    pub fn new(app: A) -> Self {
        Self {
            app,
            heartbeat: Box::new(|_| false),
            commands: Vec::new(),
            windowed: None,
            headless: None,
        }
    }

    /// Marks periodic messages (`time::every`, `window::frames`) that mean
    /// "time passed", so `wait-idle` does not count them as activity.
    pub fn heartbeat(self, is_heartbeat: impl Fn(&A::Message) -> bool + 'static) -> Self {
        Self {
            heartbeat: Box::new(is_heartbeat),
            ..self
        }
    }

    /// Runs `setup` right before the windowed app starts, on the thread that
    /// runs its event loop; never for `ctl` or `--headless`. Use it for
    /// desktop integration such as tray icons. A second call replaces the
    /// first.
    pub fn windowed(self, setup: impl FnOnce() + 'static) -> Self {
        Self {
            windowed: Some(Box::new(setup)),
            ..self
        }
    }

    /// Runs `setup` in a `--headless` process after the host flags parsed
    /// and [`is_headless`](crate::is_headless) turned true, before the app
    /// boots. Use it for headless-only setup (stub devices, argument checks
    /// via [`app_args`](crate::app_args)). A second call replaces the first.
    pub fn headless(self, setup: impl FnOnce() + 'static) -> Self {
        Self {
            headless: Some(Box::new(setup)),
            ..self
        }
    }

    /// Adds an app-specific command: `build` turns the rest of the line into
    /// a message for `update`, or an error that `ctl` prints as
    /// `err <name>: <error>`.
    ///
    /// # Panics
    ///
    /// When `name` is a built-in command.
    pub fn command(
        mut self,
        name: &'static str,
        summary: &'static str,
        build: impl Fn(&str) -> Result<A::Message, String> + 'static,
    ) -> Self {
        assert!(
            !protocol::is_builtin(name),
            "iced_impeccable: custom command \"{name}\" collides with a built-in command"
        );
        self.commands.push(Custom {
            name,
            summary,
            build: Box::new(build),
        });
        self
    }

    /// Runs the control client, the headless host, or the windowed app,
    /// depending on the command line. The first two end the process.
    pub fn run(self) -> iced::Result {
        let args: Vec<OsString> = std::env::args_os().collect();
        if args.get(1).is_some_and(|a| a == "ctl") {
            let rest: Vec<String> = args[2..]
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            std::process::exit(crate::client::run(&rest));
        }
        if args.iter().skip(1).any(|a| a == "--headless") {
            crate::HEADLESS.store(true, std::sync::atomic::Ordering::SeqCst);
            let host_args = match HostArgs::parse(&args[1..]) {
                Ok(host_args) => host_args,
                Err(e) => {
                    let bin = args
                        .first()
                        .and_then(|a| Path::new(a).file_name())
                        .map_or_else(|| "app".into(), |n| n.to_string_lossy());
                    eprintln!("[remote] {e}");
                    eprintln!("{}", args::usage(&bin));
                    std::process::exit(2);
                }
            };
            if let Some(setup) = self.headless {
                setup();
            }
            let hooks = Hooks {
                heartbeat: self.heartbeat,
                commands: self.commands,
            };
            host::run(self.app, hooks, host_args);
        }
        if let Some(setup) = self.windowed {
            setup();
        }
        Runnable::run(self.app)
    }
}
