//! # Hexdino
//!
//! A hex editor with vim like keybindings written in Rust.

#![doc(html_logo_url = "https://raw.githubusercontent.com/Luz/hexdino/master/logo.png")]

use std::io::prelude::*;
use std::fs::OpenOptions;
use std::io::SeekFrom;
use std::path::Path;
use std::error::Error;
use std::env;

mod draw;
use draw::draw;
use draw::get_absolute_draw_indices;
mod find;
use find::FindOptSubset;

extern crate ncurses;
use ncurses::*;

extern crate getopts;
use getopts::Options;

extern crate pest;
#[macro_use]
extern crate pest_derive;

use pest::Parser;

#[derive(Parser)]
#[grammar = "cmd.pest"]
struct IdentParser;

extern crate memmem;
use memmem::{Searcher, TwoWaySearcher};

#[derive(PartialEq, Copy, Clone)]
pub enum Cursorstate {
    Leftnibble,
    Rightnibble,
    Asciichar,
}

fn main() {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    let mut buf = vec![];
    let mut cursorpos: usize = 0;
    let mut cstate: Cursorstate = Cursorstate::Leftnibble;
    // 0 = display data from first line of file
    let mut screenoffset: usize = 0;
    const SPALTEN: usize = 16;
    let mut command = String::new();
    let mut debug = String::new();

    // start ncursesw
    initscr();
    let screenheight = getmaxy(stdscr()) as usize;
    // ctrl+z and fg works with this
    cbreak();
    noecho();
    start_color();
    init_pair(1, COLOR_GREEN, COLOR_BLACK);

    let args: Vec<_> = env::args().collect();
    let program = args[0].clone();
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("v", "version", "print the version");
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => {
            println!("{}", f.to_string());
            println!("Usage: {} FILE [options]", program);
            endwin();
            return;
        }
    };
    if matches.opt_present("v") {
        println!("Version: {}", VERSION);
        endwin();
        return;
    }
    if matches.opt_present("h") {
        println!("Usage: {} FILE [options]", program);
        endwin();
        return;
    }

    if !has_colors() {
        endwin();
        println!("Your terminal does not support color!\n");
        return;
    }

    let patharg = match matches.free.is_empty() {
        true => String::new(),
        false => matches.free[0].clone(),
    };
    let path = Path::new(&patharg);

    if patharg.is_empty() {
        endwin();
        println!("Patharg is empty!\n");
        return;
    }

    let mut file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&path) {
        Err(why) => {
            println!("Could not open {}: {}", path.display(), why.to_string());
            endwin();
            return;
        }
        Ok(file) => file,
    };
    file.read_to_end(&mut buf).ok().expect(
        "File could not be read.",
    );

    let draw_range = get_absolute_draw_indices(buf.len(), SPALTEN, screenoffset);
    draw(&buf[draw_range.0 .. draw_range.1], cursorpos, SPALTEN, &command, &mut debug, cstate, screenoffset);

    let mut quitnow = false;
    while quitnow == false {
        let key = std::char::from_u32(getch() as u32).unwrap();
        printw(&format!("   {:?}   ", key));
        command.push_str(&key.clone().to_string());

        let parsethisstring = command.clone();
        let commands = IdentParser::parse(Rule::cmd_list, &parsethisstring)
            .unwrap_or_else(|e| panic!("{}", e));

        let mut clear = true;
        let mut save = false;
        for cmd in commands {
            match cmd.as_rule() {
                Rule::down => {
                    printw(&format!("{:?}", cmd.as_rule()));
                    if cursorpos + SPALTEN < buf.len() {
                        // not at end
                        cursorpos += SPALTEN;
                    } else {
                        // when at end
                        if buf.len() != 0 {
                            // Suppress underflow
                            cursorpos = buf.len() - 1;
                        }
                    }
                }
                Rule::up => {
                    if cursorpos >= SPALTEN {
                        cursorpos -= SPALTEN;
                    }
                }
                Rule::left => {
                    if cstate == Cursorstate::Asciichar {
                        if cursorpos > 0 {
                            cursorpos -= 1;
                        }
                    } else if cstate == Cursorstate::Rightnibble {
                        cstate = Cursorstate::Leftnibble;
                    } else if cstate == Cursorstate::Leftnibble {
                        if cursorpos > 0 {
                            // not at start
                            cstate = Cursorstate::Rightnibble;
                            cursorpos -= 1;
                        }
                    }
                }
                Rule::right => {
                    if cstate == Cursorstate::Asciichar {
                        if cursorpos + 1 < buf.len() {
                            // not at end
                            cursorpos += 1;
                        }
                    } else if cstate == Cursorstate::Leftnibble {
                        cstate = Cursorstate::Rightnibble;
                    } else if cstate == Cursorstate::Rightnibble {
                        if cursorpos + 1 < buf.len() {
                            // not at end
                            cstate = Cursorstate::Leftnibble;
                            cursorpos += 1;
                        }
                    }
                }
                Rule::start => {
                    cursorpos -= cursorpos % SPALTEN; // jump to start of line
                    if cstate == Cursorstate::Rightnibble {
                        cstate = Cursorstate::Leftnibble;
                    }
                }
                Rule::end => {
                    // check if no overflow
                    if cursorpos - (cursorpos % SPALTEN) + (SPALTEN - 1) < buf.len() {
                        // jump to end of line
                        cursorpos = cursorpos - (cursorpos % SPALTEN) + (SPALTEN - 1);
                    } else {
                        // jump to end of line
                        cursorpos = buf.len() - 1
                    }
                    if cstate == Cursorstate::Leftnibble {
                        cstate = Cursorstate::Rightnibble;
                    }
                }
                Rule::top => {
                    cursorpos = 0;
                }
                Rule::bottom => {
                    cursorpos = buf.len() - 1;
                    cursorpos -= cursorpos % SPALTEN; // jump to start of line
                }
                Rule::replace => {
                    // printw("next char will be the replacement!");
                    clear = false;
                }
                Rule::remove => {
                    // check if in valid range
                    if buf.len() > 0 && cursorpos < buf.len() {
                        // remove the current char
                        buf.remove(cursorpos);
                    }
                    // always perform the movement if possible
                    if cursorpos > 0 && cursorpos >= buf.len() {
                        cursorpos -= 1;
                    }
                }
                Rule::insert => {
                    printw("next chars will be inserted!");
                    clear = false;
                }
                Rule::jumpascii => {
                    if cstate == Cursorstate::Asciichar {
                        cstate = Cursorstate::Leftnibble;
                    } else {
                        cstate = Cursorstate::Asciichar;
                    }
                }
                Rule::helpfile => {
                    command.push_str("No helpfile yet");
                }
                Rule::backspace => {
                    command.pop();
                    command.pop();
                    clear = false;
                }
                Rule::saveandexit => {
                    save = true;
                    quitnow = true;
                }
                Rule::exit => quitnow = true,
                Rule::save => save = true,

                _ => (),
            }

            for inner_cmd in cmd.into_inner() {
                match inner_cmd.as_rule() {
                    Rule::replacement => {
                        // TODO: use inner_cmd and not just "key"
                        // printw(&format!("Replacement: {:?}", inner_cmd.as_str()));
                        if cstate == Cursorstate::Asciichar {
                            if cursorpos >= buf.len() {
                                buf.insert(cursorpos, 0);
                            }
                            // buf[cursorpos] = inner_cmd.as_str();
                            buf[cursorpos] = key as u8;
                        } else {
                            let mask = if cstate == Cursorstate::Leftnibble {
                                0x0F
                            } else {
                                0xF0
                            };
                            let shift = if cstate == Cursorstate::Leftnibble {
                                4
                            } else {
                                0
                            };
                            if cursorpos >= buf.len() {
                                buf.insert(cursorpos, 0);
                            }
                            // Change the selected nibble
                            if let Some(c) = key.to_digit(16) {
                                buf[cursorpos] = buf[cursorpos] & mask | (c as u8) << shift;
                            }
                        }

                    }
                    // TODO: use inner_cmd and not just "key"
                    Rule::insertment => {
                        // printw(&format!("Inserted: {:?}", inner_cmd.as_str()));
                        command.pop(); // remove the just inserted thing
                        clear = false;

                        if cstate == Cursorstate::Leftnibble {
                            // Left nibble
                            if let Some(c) = (key as char).to_digit(16) {
                                buf.insert(cursorpos, (c as u8) << 4);
                                cstate = Cursorstate::Rightnibble;
                            }
                        } else if cstate == Cursorstate::Rightnibble {
                            // Right nibble
                            if cursorpos == buf.len() {
                                buf.insert(cursorpos, 0);
                            }
                            if let Some(c) = (key as char).to_digit(16) {
                                buf[cursorpos] = buf[cursorpos] & 0xF0 | c as u8;
                                cstate = Cursorstate::Leftnibble;
                                cursorpos += 1;
                            }
                        } else if cstate == Cursorstate::Asciichar {
                            buf.insert(cursorpos, key as u8);
                            cursorpos += 1;
                        }
                    }
                    Rule::searchstr => {
                        let search = inner_cmd.as_str().as_bytes();
                        let foundpos = TwoWaySearcher::new(&search);
                        cursorpos = foundpos.search_in(&buf).unwrap_or(cursorpos);
                    }
                    Rule::searchbytes => {
                        let search = inner_cmd.as_str().as_bytes();
                        let mut needle = vec![];
                        for i in 0..search.len() {
                            let nibble = match search[i] as u8 {
                                c @ 48...57 => c - 48, // Numbers from 0 to 9
                                b'x' => 0x10, // x is the wildcard
                                b'X' => 0x10, // X is the wildcard
                                c @ b'a'...b'f' => c - 87,
                                c @ b'A'...b'F' => c - 55,
                                _ => panic!("Should not get to this position!"),
                            };
                            needle.push(nibble);
                        }
                        cursorpos = buf.find_subset(&needle).unwrap_or(cursorpos);
                        // endwin(); println!("Searching for: {:?}", needle ); return;
                    }
                    Rule::linenumber => {
                        let linenr: usize = inner_cmd.as_str().parse().unwrap();
                        cursorpos = linenr * SPALTEN; // jump to the line
                        if cursorpos > buf.len() { // detect file end
                            cursorpos = buf.len();
                        }
                        cursorpos -= cursorpos % SPALTEN; // jump to start of line
                    }
                    Rule::escape => (),
                    Rule::gatherone => clear = false,
                    _ => {
                        command.push_str(&format!("no rule for {:?} ", inner_cmd.as_rule()));
                        clear = false;
                    }
                };
            }
            if save {
                if path.exists() {
                    let mut file = match OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create(true)
                        .open(&path) {
                        Err(why) => {
                            panic!(
                                "Could not open {}: {}",
                                path.display(),
                                Error::description(&why)
                            )
                        }
                        Ok(file) => file,
                    };
                    file.seek(SeekFrom::Start(0)).ok().expect(
                        "Filepointer could not be set to 0",
                    );
                    file.write_all(&mut buf).ok().expect(
                        "File could not be written.",
                    );
                    file.set_len(buf.len() as u64).ok().expect(
                        "File could not be set to correct lenght.",
                    );
                    command.push_str("File saved!");
                } else {
                    command.push_str("Careful, file could not be saved!");
                }
                // TODO: define filename during runtime
                save = false;
            }
            if clear {
                command.clear();
            }

            // Always move screen when cursor leaves screen
            if cursorpos > (screenheight + screenoffset - 1) * SPALTEN - 1 {
                screenoffset = 2 + cursorpos / SPALTEN - screenheight;
            }
            if cursorpos < screenoffset * SPALTEN {
                screenoffset = cursorpos / SPALTEN;
            }


        }

    let draw_range = get_absolute_draw_indices(buf.len(), SPALTEN, screenoffset);
        draw(&buf[draw_range.0 .. draw_range.1], cursorpos, SPALTEN, &command, &mut debug, cstate, screenoffset);
    }

    refresh();
    endwin();
}
