//! Mini opinionated logging utilities.
//! Spawns of separate logging thread so you don't get held back by silly formatting on the main thread

use crossbeam_channel::{Receiver, Sender, unbounded};
#[doc(hidden)]
pub use jiff::Zoned;
use owo_colors::*;
// #[doc(hidden)]
// pub use quanta::{Clock, Instant};
use std::{
    any::Any,
    borrow::Cow,
    cell::RefCell,
    fmt::{Display, Write},
    io::stderr,
    marker::PhantomData,
    panic::AssertUnwindSafe,
    sync::{Condvar, LazyLock, Mutex, PoisonError, atomic::AtomicU8},
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Off,
}
impl Level {
    pub fn text(&self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO ",
            Self::Warn => "WARN ",
            Self::Error => "ERROR",
            Self::Off => "",
        }
    }
}
impl Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Trace => write!(f, "{}", self.text().purple().bold()),
            Self::Debug => write!(f, "{}", self.text().blue().bold()),
            Self::Info => write!(f, "{}", self.text().green().bold()),
            Self::Warn => write!(f, "{}", self.text().yellow().bold()),
            Self::Error => write!(f, "{}", self.text().red().bold()),
            Self::Off => Ok(()),
        }
    }
}

thread_local! {
    #[doc(hidden)]
    pub static SPAN_STACK: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

pub struct Span(&'static str);
pub struct SpanEnterGuard<'a>(PhantomData<&'a mut Span>);
impl Span {
    pub fn new(string: &'static str) -> Self {
        Self(string)
    }
    /// Enters the span
    #[must_use = "The span must be used to stay alive"]
    pub fn enter(&mut self) -> SpanEnterGuard<'_> {
        SPAN_STACK.with_borrow_mut(|x| x.push(self.0));
        SpanEnterGuard(PhantomData)
    }
}
impl Drop for SpanEnterGuard<'_> {
    fn drop(&mut self) {
        SPAN_STACK.with_borrow_mut(|x| _ = x.pop().expect("uh oh"))
    }
}

const SPACES: &str = unsafe { std::str::from_utf8_unchecked(&[b' '; 256]) };

struct InternalWriter<W: Write> {
    prefix_written: bool,
    prefix_size: usize,
    writer: W,
}

impl<W: Write> InternalWriter<W> {
    fn new(writer: W) -> Self {
        Self {
            prefix_size: 0,
            prefix_written: false,
            writer,
        }
    }
    fn set_written_prefix(&mut self) {
        self.prefix_written = true;
    }
}
impl<W: Write> Write for InternalWriter<W> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        if !self.prefix_written {
            self.prefix_size += ansi_width::ansi_width(s);
            self.writer.write_str(s)
        } else {
            let mut lines = s.split_inclusive('\n');
            let Some(first) = lines.next() else {
                return Ok(());
            };
            self.writer.write_str(first)?;
            for line in lines {
                self.writer.write_str(&SPACES[..self.prefix_size])?;
                self.writer.write_str(line)?;
            }
            Ok(())
        }
    }
}

type Writer<'a> = InternalWriter<&'a mut (dyn Write + Send + Sync + 'static)>;

pub struct LogInfo {
    pub level: Level,
    pub module_path: &'static str,
    pub spans: Vec<&'static str>,
    pub last_entry_elapsed: u64,
}

struct LogMessage<T: ?Sized + Any> {
    pub info: LogInfo,
    pub payload: T,
}

pub trait LogFormatter {
    fn format_prefix(&self, writer: &mut impl Write, message: &LogInfo) -> std::fmt::Result;
}

const MODULE_FLAG: u8 = 1;
const SPAN_FLAG: u8 = 1 << 1;
const ELAPSED_FLAG: u8 = 1 << 2;

pub struct DefaultLogFormatter {
    flags: u8,
    time_format: Cow<'static, str>,
}

impl Default for DefaultLogFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultLogFormatter {
    pub fn new() -> Self {
        DefaultLogFormatter {
            flags: MODULE_FLAG | SPAN_FLAG | ELAPSED_FLAG,
            time_format: "%F %T.%6f".into(),
        }
    }
    /// Changes the format of the time string. See [`jiff::fmt::strtime`].
    pub fn with_time_format(self, time_format: Cow<'static, str>) -> Self {
        Self {
            flags: self.flags,
            time_format,
        }
    }
    /// Removes the module path from output
    pub fn without_module_path(self) -> Self {
        Self {
            flags: self.flags & !MODULE_FLAG,
            time_format: self.time_format,
        }
    }
    /// Removes the elapsed time for reentrant events from the log message
    pub fn without_elapsed_time(self) -> Self {
        Self {
            flags: self.flags & !ELAPSED_FLAG,
            time_format: self.time_format,
        }
    }
}

impl LogFormatter for DefaultLogFormatter {
    fn format_prefix(&self, writer: &mut impl Write, message: &LogInfo) -> std::fmt::Result {
        if !self.time_format.is_empty() {
            let time = Zoned::now();
            write!(
                writer,
                "{} ",
                time.strftime(self.time_format.as_bytes()).dimmed()
            )?;
        }
        write!(writer, "{} ", message.level)?;
        if self.flags & MODULE_FLAG != 0 {
            write!(writer, "{} ", message.module_path.dimmed())?;
        }
        if self.flags & ELAPSED_FLAG != 0 && message.last_entry_elapsed != 0 {
            write!(
                writer,
                "{}({:.03?} elapsed)\x1b[0m ",
                owo_colors::colors::BrightBlack::ANSI_FG,
                Duration::from_nanos(message.last_entry_elapsed),
            )?;
        }

        if !message.spans.is_empty() && self.flags & SPAN_FLAG != 0 {
            for component in message.spans.iter() {
                write!(writer, "{}:", component.italic())?;
            }
            writer.write_str(" ")?;
        }

        Ok(())
    }
}

#[doc(hidden)]
pub static LOG_LEVEL: AtomicU8 = AtomicU8::new(Level::Info as _);
#[derive(Clone, Debug, Copy)]
pub struct StderrWriter;
impl std::fmt::Write for StderrWriter {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        use std::io::Write as _;
        stderr()
            .write_all(s.as_bytes())
            .map_err(|_| std::fmt::Error)
    }
}

trait ErasedLogFormatter {
    fn format_prefix(&self, writer: &mut Writer<'_>, message: &LogInfo) -> std::fmt::Result;
}
impl<T: LogFormatter> ErasedLogFormatter for T {
    fn format_prefix(&self, writer: &mut Writer<'_>, message: &LogInfo) -> std::fmt::Result {
        <T as LogFormatter>::format_prefix(self, writer, message)
    }
}
struct GlobalOptions {
    formatter: Box<dyn ErasedLogFormatter + Send + Sync + 'static>,
    writer: Box<dyn Write + Send + Sync + 'static>,
}
static GLOBAL_OPTIONS: Mutex<Option<GlobalOptions>> = Mutex::new(None);

static LOG_CHANNEL: LazyLock<(Sender<M>, Receiver<M>)> = LazyLock::new(unbounded);
static STOP_LOCK: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

trait LogPayload: Display + Send + Sync + Any {
    fn is_stop(&self) -> bool {
        self.type_id() == std::any::TypeId::of::<StopLogging>()
    }
}

impl<T> LogPayload for T where T: Display + Send + Sync + Any {}
type M = Box<LogMessage<dyn LogPayload>>;

fn process_log_message(msg: &LogMessage<dyn LogPayload>) -> std::fmt::Result {
    // we don't care if the format code panics. just keep doing our thing...
    let mut options = GLOBAL_OPTIONS
        .lock()
        .unwrap_or_else(PoisonError::into_inner);

    if let Some(GlobalOptions { formatter, writer }) = options.as_mut() {
        let mut writer = InternalWriter::new(&mut **writer);
        formatter.format_prefix(&mut writer, &msg.info)?;
        writer.set_written_prefix();
        writeln!(writer, "{}", &msg.payload)
    } else {
        Ok(())
    }
}

struct StopLogging;
impl Display for StopLogging {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}
static LOG_THREAD: LazyLock<JoinHandle<()>> = LazyLock::new(|| {
    std::thread::spawn(|| {
        loop {
            let msg = LOG_CHANNEL.1.recv().unwrap();
            if msg.payload.is_stop() {
                *STOP_LOCK.0.lock().unwrap_or_else(PoisonError::into_inner) = false;
                STOP_LOCK.1.notify_all();
                break;
            }
            GLOBAL_OPTIONS.clear_poison();
            _ = std::panic::catch_unwind(AssertUnwindSafe(move || process_log_message(&msg)));
        }
    })
});

#[doc(hidden)]
pub fn send_log_message<T: Display + Send + Sync + 'static>(info: LogInfo, payload: T) {
    // process_log_message(&LogMessage { info, payload }).unwrap();
    _ = LOG_CHANNEL
        .0
        .try_send(Box::new(LogMessage { info, payload }));
}

/// Parses the level from a string
pub fn parse_level(value: &str) -> Option<Level> {
    Some(match value.to_lowercase().trim_ascii() {
        "trace" => Level::Trace,
        "debug" => Level::Debug,
        "info" => Level::Info,
        "warn" => Level::Warn,
        "error" => Level::Error,
        "off" | "none" => Level::Off,
        _ => return None,
    })
}

/// Sets the global log level
pub fn set_level(level: Level) {
    LOG_LEVEL.store(level as _, std::sync::atomic::Ordering::Relaxed);
}
pub fn set_writer_and_format<W, F>(writer: W, formatter: F)
where
    W: Write + Send + Sync + 'static,
    F: LogFormatter + Send + Sync + 'static,
{
    let mut guard = GLOBAL_OPTIONS.lock().unwrap();
    guard.replace(GlobalOptions {
        formatter: Box::new(formatter),
        writer: Box::new(writer),
    });
}

#[must_use = "The logging thread must be kept alive to do the logging thing"]
pub struct LoggingThread;
impl Drop for LoggingThread {
    fn drop(&mut self) {
        let mut stopping = STOP_LOCK.0.lock().unwrap_or_else(PoisonError::into_inner);
        send_log_message(
            LogInfo {
                level: Level::Error,
                module_path: "",
                spans: Vec::new(),
                last_entry_elapsed: 0,
            },
            StopLogging,
        );
        *stopping = true;
        while *stopping {
            stopping = STOP_LOCK
                .1
                .wait(stopping)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// Initializes the logging system by spawning the logging thread.
pub fn init() -> LoggingThread {
    if LOG_LEVEL.load(std::sync::atomic::Ordering::Relaxed) != Level::Off as _ {
        _ = *LOG_THREAD;
    }
    LoggingThread
    // start the log thread
}

#[macro_export]
#[doc(hidden)]
macro_rules! make_newline_separated {
    ($first:literal $(, $rest:literal)*) => {
        concat!($first $(, "\n", $rest)*)
    };
}
#[doc(hidden)]
pub static FIRST_LOAD_TIME: LazyLock<Instant> = LazyLock::new(Instant::now);
#[doc(hidden)]
#[macro_export]
macro_rules! log_inner {
    ($reenter:literal, $level:expr $(, $fmt:literal)+ $(, $field:ident = $e:expr)*) => {{
        static ඞLAST_MEASUREMENTඞ: ::std::sync::atomic::AtomicU64 = ::std::sync::atomic::AtomicU64::new(0);

        let ඞlevelඞ = $level;
        if  ඞlevelඞ as ::std::primitive::u8 >= $crate::LOG_LEVEL.load(::std::sync::atomic::Ordering::Relaxed) {
            let ඞlog_infoඞ = $crate::LogInfo {
                level: ඞlevelඞ,
                module_path: ::std::module_path!(),
                // TODO: get rid of this pesky extra heap alloc...
                spans: $crate::SPAN_STACK.with_borrow(|x| x.to_vec()),
                last_entry_elapsed: if $reenter {
                    let new_measurement = $crate::FIRST_LOAD_TIME.elapsed().as_nanos() as u64;
                    let last_measurement = ඞLAST_MEASUREMENTඞ.swap(new_measurement, ::std::sync::atomic::Ordering::AcqRel);
                    if  last_measurement == 0 { 0 } else { new_measurement - last_measurement}
                } else {
                    0
                },
            };
            let ඞpayloadඞ = {
                $(let $field = $e;)*
                ::std::fmt::from_fn(move |ඞ| ::std::write!(ඞ,
                    $crate::make_newline_separated!($($fmt),+)
                    $(, $field = $field)*))
            };
            $crate::send_log_message(ඞlog_infoඞ, ඞpayloadඞ);
        }

    }};
}

#[macro_export]
macro_rules! log {
    ($level:expr $(, $fmt:literal)+ $(, $field:ident = $e:expr)*) => {
        $crate::log_inner!(false, $level $(, $fmt)+ $(, $field = $e)*)
    };
}

#[macro_export]
macro_rules! log_reenter {
    ($level:expr $(, $fmt:literal)+ $(, $field:ident = $e:expr)*) => {
        $crate::log_inner!(true, $level $(, $fmt)+ $(, $field = $e)*)
    };
}

macro_rules! define_log_macros {
    ($dollar:tt;
    $(
        $name:ident =
        $reentrant:literal
        $level:ident
    ),*
    ) => {
        $(
        #[doc = concat!("Logs a message at the ")]
        #[macro_export]
        macro_rules! $name {
            ($dollar ($dollar fmt:literal),+ $dollar (, $dollar field:ident = $dollar e:expr)*) => {
                $crate::log_inner!($reentrant, $crate::Level::$level $dollar(, $dollar fmt)+ $dollar(, $dollar field = $dollar e)*)
            };
        })*
    };
}

define_log_macros! {
    $;
    trace = false Trace,
    trace_reenter = true Trace,
    debug = false Debug,
    debug_reenter = true Debug,
    info = false Info,
    info_reenter = true Info,
    warn = false Warn,
    warn_reenter = true Warn,
    error = false Error,
    error_reenter = true Error
}
