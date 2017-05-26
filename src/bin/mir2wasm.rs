#![feature(rustc_private, link_args)]

extern crate env_logger;
extern crate getopts;
#[macro_use]
extern crate log;
extern crate mir2wasm;
extern crate rustc;
extern crate rustc_driver;

use getopts::{getopts, optflag, optopt};
use mir2wasm::trans::{self, WasmTransOptions};
use rustc::session::Session;
use rustc_driver::{driver, CompilerCalls};
use std::process;

struct WasmCompilerCalls {
    options: WasmTransOptions,
}

impl WasmCompilerCalls {
    fn new(options: WasmTransOptions) -> WasmCompilerCalls {
        WasmCompilerCalls { options: options }
    }
}

impl<'a> CompilerCalls<'a> for WasmCompilerCalls {
    fn build_controller(&mut self,
                        _: &Session,
                        _: &getopts::Matches)
                        -> driver::CompileController<'a> {
        let mut control = driver::CompileController::basic();

        let options = self.options.clone();

        control.after_analysis.stop = rustc_driver::Compilation::Stop;
        control.after_analysis.callback = Box::new(move |state| {
            state.session.abort_if_errors();

            let entry_fn = state.session.entry_fn.borrow();
            let entry_fn = if let Some((node_id, _)) = *entry_fn {
                Some(node_id)
            } else {
                None
            };
            trans::trans_crate(state.tcx.expect("type context needed").global_tcx(),
                               entry_fn,
                               &options)
                .expect("error translating crate")
        });

        control
    }
}

fn main() {
    env_logger::init().unwrap();

    let opts = &[optflag("r",
                         "run",
                         "run the compiled module through the interpreter, without printing it"),
                 optopt("o", "", "write a binary wasm module to FILE", "FILE"),
                 optflag("O", "", "optimize the compiled wast module"),
                 optflag("q", "", "do not print the compiled wast module"),
                 optflag("", "trace", "trace binaryen api calls"),
                 optflag("h", "help", "display this help message")];

    let mut rustc_args = Vec::new();
    let mut wasm_args = Vec::new();

    fn find_wasm_arg<'a>(s: &String,
                         opts: &'a [getopts::OptGroup])
                         -> Option<&'a getopts::OptGroup> {
        for o in opts {
            if s.starts_with("--") && &s[2..] == &o.long_name {
                return Some(o);
            }
            if s.starts_with("-") && &s[1..] == &o.short_name {
                return Some(o);
            }
        }
        return None;
    };

    let args: Vec<String> = std::env::args().collect();
    info!("command line: {:?}", args);

    let mut argv = std::env::args().peekable();
    loop {
        match argv.next() {
            Some(arg) => {
                match find_wasm_arg(&arg, opts) {
                    Some(opt) => {
                        wasm_args.push(arg);

                        match opt.hasarg {
                            getopts::HasArg::Yes => {
                                wasm_args.push(argv.next().expect("missing required argument"))
                            }
                            getopts::HasArg::No => (),
                            getopts::HasArg::Maybe => {
                                if argv.peek().map_or(false, |s| s.starts_with("-")) {
                                    wasm_args.push(argv.next().expect("this was just here"));
                                }
                            }
                        }
                    }
                    None => rustc_args.push(arg),
                }
            }
            None => break,
        }
    }
    info!("wasm args: {:?}", wasm_args);
    info!("rustc args: {:?}", rustc_args);

    let mut options = WasmTransOptions::new();

    let matches = getopts(&wasm_args[..], opts).expect("could not parse command line arguments");

    if matches.opt_present("h") {
        let brief = format!("Usage: {} [options]", args[0]);
        print!("{}", getopts::usage(brief.as_str(), opts));
        return;
    }
    if matches.opt_present("r") {
        options.interpret = true;
    }
    if matches.opt_present("o") {
        options.binary_output_path = matches.opt_str("o");
    }
    if matches.opt_present("O") {
        options.optimize = true;
    }
    if matches.opt_present("q") {
        options.print = false;
    }
    if matches.opt_present("trace") {
        options.trace = true;
    }

    let mut compiler_calls = WasmCompilerCalls::new(options);
    match rustc_driver::run_compiler(&rustc_args, &mut compiler_calls, None, None) {
        (Ok(_), _) => process::exit(0),
        (Err(code), _) => process::exit(code as i32),
    }
}
