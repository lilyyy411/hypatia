use color_backtrace::BacktracePrinter;
use gtk4::{
    Application, ApplicationWindow, GLArea,
    gdk::GLContext,
    gio::prelude::{ApplicationExt, ApplicationExtManual},
    glib::{ExitCode, Propagation},
    prelude::{GLAreaExt, GtkWindowExt},
};
use indenter::{Indented, indented};
use libloading::os::unix::Library;
use linkme::distributed_slice;
use macro_rules_attribute::apply;
use mpv_gl_renderer::{render::RenderContextInitParams, *};
use owo_colors::OwoColorize;

use std::{
    fmt::Write as _,
    io::{Stdout, Write, stdout},
    panic::{AssertUnwindSafe, PanicHookInfo},
    sync::Arc,
};
use termcolor::Ansi;
#[macro_export]
macro_rules! test {
    {$(fn $name:ident($mpv:ident: _) $block:block)*} => {
        $(mod $name {
            const NAME: &str = module_path!();
            #[allow(non_upper_case_globals)]
            #[linkme::distributed_slice($crate::TESTS)]
            static $name: $crate::Test = $crate::Test {
                name: NAME,
                func: super::$name
            };
        }
        fn $name($mpv: MpvHandle<std::sync::Arc<MpvContext>>) -> eyre::Result<()> {  $block; Ok(())}
        )*
    };
}
#[macro_export]
macro_rules! test_module {
    (mod $name:ident { $($t:tt)* }) => {
        #[allow(unused)]
        mod $name {
            use macro_rules_attribute::apply;
            use mpv_gl_renderer::*;
            use pretty_assertions::{assert_eq, assert_ne};
            use $crate::{test, test_module};
             $($t)*
        }
    };
}

#[apply(test!)]
fn a_doesnt_explode(_mpv: _) {}
#[apply(test_module!)]
mod property {
    #[apply(test_module!)]
    mod default {
        #[apply(test!)]
        fn gpu_api_opengl(mpv: _) {
            assert_eq!(mpv.get_prop::<MpvByteString>(c"gpu-api")?, c"opengl");
        }
        #[apply(test!)]
        fn vo_libmpv(mpv: _) {
            assert_eq!(mpv.get_prop::<MpvByteString>(c"vo")?, c"libmpv")
        }

        #[apply(test!)]
        fn hw_decode_auto(mpv: _) {
            assert_eq!(mpv.get_prop::<MpvByteString>(c"hwdec")?, c"auto");
        }
    }
    #[apply(test_module!)]
    mod get_set {
        #[apply(test!)]
        fn f64(mpv: _) {
            mpv.set_prop(c"volume", 90.0f64)?;
            assert_eq!(mpv.get_prop::<f64>(c"volume")?, 90.0);
        }
        #[apply(test!)]
        fn f64_as_f32(mpv: _) {
            mpv.set_prop(c"volume", 90.0f32)?;
            assert_eq!(mpv.get_prop::<f64>(c"volume")?, 90.0);
        }
        #[apply(test!)]
        fn flag_false(mpv: _) {
            mpv.set_prop(c"subs-match-os-language", false)?;
            assert!(!mpv.get_prop::<bool>(c"subs-match-os-language")?);
        }
        #[apply(test!)]
        fn flag_true(mpv: _) {
            mpv.set_prop(c"subs-match-os-language", true)?;
            assert!(mpv.get_prop::<bool>(c"subs-match-os-language")?);
        }
        #[apply(test!)]
        fn string(mpv: _) {
            mpv.set_prop(c"loop", c"inf")?;
            assert_eq!(mpv.get_prop::<MpvByteString>(c"loop")?, c"inf");
        }
        #[apply(test!)]
        fn i64(mpv: _) {
            mpv.set_prop(c"loop", 10i64)?;
            assert_eq!(mpv.get_prop::<i64>(c"loop")?, 10);
        }
        #[apply(test!)]
        fn i64_as_i32(mpv: _) {
            mpv.set_prop(c"loop", 10i32)?;
            assert_eq!(mpv.get_prop::<i64>(c"loop")?, 10);
        }
        #[apply(test!)]
        fn i64_as_i16(mpv: _) {
            mpv.set_prop(c"loop", 10i16)?;
            assert_eq!(mpv.get_prop::<i64>(c"loop")?, 10);
        }
        #[apply(test!)]
        fn i64_as_i8(mpv: _) {
            mpv.set_prop(c"loop", 10i8)?;
            assert_eq!(mpv.get_prop::<i64>(c"loop")?, 10);
        }
        #[apply(test!)]
        fn flag2_electric_boogaloo(mpv: _) {
            mpv.set_prop(c"loop", c"no")?;
            assert!(!mpv.get_prop(c"loop")?);
        }
    }
    #[apply(test_module!)]
    mod errors {
        #[apply(test!)]
        fn f64_as_flag(mpv: _) {
            mpv.set_prop(c"volume", 90.0f64)?;
            assert_eq!(
                mpv.get_prop::<bool>(c"volume"),
                Err(error::Error::PropertyFormat)
            );
        }
        #[apply(test!)]
        fn string_as_int(mpv: _) {
            // yes, properties can have different types depending on the value lol
            mpv.set_prop(c"loop", c"inf")?;
            assert_eq!(
                mpv.get_prop::<i64>(c"loop"),
                Err(error::Error::PropertyFormat)
            );
        }
        #[apply(test!)]
        fn non_existent_property(mpv: _) {
            assert_eq!(
                mpv.get_prop::<MpvByteString>(c"catbirl"),
                Err(error::Error::PropertyNotFound)
            )
        }
    }
}

/* The test harness */
pub fn main() -> ExitCode {
    _ = init_gl();
    if std::env::var("TELL_ME_WHAT_WENT_WRONG").is_err() {
        // Shut THE FUCK UP ffmpeg, I DON'T CARE if you can't load libcuda.so.1.
        // You're ruining the aesthetic of my test suite.
        unsafe {
            let redirect = libc::open(c"/dev/null".as_ptr(), 0);
            libc::dup2(redirect, libc::STDERR_FILENO);
        }
    }
    std::panic::set_hook(Box::new(panic_hook));
    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(activate);
    app.run()
}
fn init_gl() -> Library {
    let library = unsafe { libloading::os::unix::Library::new("libepoxy.so.0") }.unwrap();

    epoxy::load_with(|name| {
        // dbg!(name);
        unsafe { library.get::<_>(name.as_bytes()) }
            .map(|symbol| *symbol)
            .unwrap_or(std::ptr::null())
    });

    library
}
const APP_ID: &str = "com.github.lilyyy411.MpvGlRenderer.tests";

fn activate(app: &Application) {
    let window = ApplicationWindow::new(app);
    // let app = app.clone();
    let area = GLArea::new();
    area.make_current();
    area.connect_render(move |_, ctx| do_tests(ctx));
    window.set_child(Some(&area));
    window.present();
}

fn do_tests(_ctx: &GLContext) -> Propagation {
    unsafe {
        libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
    }
    let mut successes = 0;
    let mut tests = TESTS.to_vec();
    tests.sort_by_key(|x| x.name);
    let longest_test_name_length = tests
        .iter()
        .max_by_key(|x| x.name.len())
        .unwrap()
        .name
        .len();
    let num_tests = tests.len();

    for test in tests {
        successes += run_test(&test, longest_test_name_length) as u32;
    }
    println!();

    if successes as usize != num_tests {
        println!(
            "{} {} {} {}",
            successes.red().bold(),
            "out of".bright_red().dimmed(),
            num_tests.red().bold(),
            "tests passed".bright_red().dimmed()
        );
        std::process::exit(1)
    } else {
        println!(
            "{} {} {}",
            "All".green().bold(),
            num_tests.green().bold(),
            "tests passed".green().bold()
        );
        std::process::exit(0)
    }
}

#[derive(Clone, Copy)]
pub struct Test {
    name: &'static str,
    func: fn(MpvHandle<Arc<MpvContext>>) -> eyre::Result<()>,
}

#[distributed_slice]
static TESTS: [Test];

struct CoolerStdout(Stdout);

impl std::fmt::Write for CoolerStdout {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.write_all(s.as_bytes()).map_err(|_| std::fmt::Error)
    }
    fn write_fmt(&mut self, args: std::fmt::Arguments<'_>) -> std::fmt::Result {
        self.0.write_fmt(args).map_err(|_| std::fmt::Error)
    }
}

struct IndentedStdout<'a>(Indented<'a, CoolerStdout>);
impl std::io::Write for IndentedStdout<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let str = std::str::from_utf8(buf).expect("test output should not contain invalid utf8");
        self.0.write_str(str).unwrap();
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn panic_hook(info: &PanicHookInfo<'_>) {
    println!("{}", "FAILED".red().bold());
    let mut writer = CoolerStdout(stdout());
    let writer = indented(&mut writer);
    let mut writer = Ansi::new(IndentedStdout(writer));

    _ = BacktracePrinter::new().print_panic_info(info, &mut writer);
}

pub fn run_test(Test { name, func }: &Test, longest_name_length: usize) -> bool {
    print!(
        "Running {name}... {}",
        " ".repeat(longest_name_length - name.len())
    );
    std::io::stdout().flush().unwrap();
    let params = RenderContextInitParams::builder()
        .symbol_lookup(|x| epoxy::get_proc_addr(x.to_str().unwrap()).cast_mut())
        .update_callback(|| {})
        .build();
    let mpv = MpvContext::new().unwrap();
    let (mpv, _) = mpv.make_render_context(Arc::new, params).unwrap();
    let e = std::panic::catch_unwind(AssertUnwindSafe(|| func(mpv.clone())));
    match e {
        Err(_) => false,
        Ok(Err(e)) => {
            println!("{}", "FAILED".red().bold());
            println!("    Test returned error: {e:?}");
            false
        }
        Ok(Ok(())) => {
            println!("{}", "SUCCESS".green().bold());
            true
        }
    }
}
