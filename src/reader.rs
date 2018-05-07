use std::borrow::Cow;

use libc::STDIN_FILENO;

use nix;
use nix::sys::termios::{tcgetattr, tcsetattr, SetArg, LocalFlags, InputFlags,
                        SpecialCharacterIndices};
use nix::sys::select::{select, FdSet};
use nix::unistd::read;

use term::Term;
use history::History;

pub struct Reader {
    term: Term,
    history: History,
    bindings: Vec<(Cow<'static, [u8]>, Keybind)>,
}

impl Reader {
    pub fn new(prompt: String) -> Self {
        settings_term();
        Reader {
            term: Term::new(prompt),
            history: History::new(),
            bindings: bindings(),
        }
    }

    pub fn read_line(&mut self) -> String {
        self.term.write_prompt().unwrap();
        loop {
            if wait_input() {
                let mut ch: Vec<u8> = Vec::new();
                let _ = self.read_char(&mut ch).unwrap();
                let res = self.find_bind(&ch);
                if let Some(line) = self.execute_sequence(res, ch) {
                    return line;
                }
            }
        }
    }

    fn execute_sequence(&mut self, res: Option<Keybind>, ch: Vec<u8>) -> Option<String> {
        match res {
            Some(Keybind::Enter) => {
                let result = self.term.line.clone();
                self.term.reset();
                self.term.new_line().unwrap();
                self.history.push(result.clone());
                self.history.reset();
                return Some(result);
            }
            Some(Keybind::CtrlL) => {
                self.term.clear_screen().unwrap();
                self.term.write_line().unwrap();
                return None;
            }
            Some(Keybind::Delete) => {
                self.term.delete(1).unwrap();
                return None;
            }
            Some(Keybind::ForwardChar) => {
                self.term.move_right(1).unwrap();
                return None;
            }
            Some(Keybind::BackwardChar) => {
                self.term.move_left(1).unwrap();
                return None;
            }
            Some(Keybind::PreviousHistory) => {
                if self.history.is_started() {
                    self.history.set_first(self.term.line.clone());
                }
                let history = match self.history.prev() {
                    Some(h) => h,
                    None => return None,
                };
                self.term.rewrite(history).unwrap();
                self.term.move_to_end().unwrap();
                return None;
            }
            Some(Keybind::NextHistory) => {
                let history = match self.history.next() {
                    Some(h) => h,
                    None => return None,
                };
                self.term.rewrite(history).unwrap();
                self.term.move_to_end().unwrap();
                return None;
            }
            Some(Keybind::Something) => {
                return None;
            }
            None => {
                self.term.put(String::from_utf8(ch).unwrap()).unwrap();
                self.history.reset_first();
                return None;
            }
        }
    }

    fn read_char(&self, buf: &mut Vec<u8>) -> nix::Result<usize> {
        buf.reserve(32);

        let len = buf.len();
        let cap = buf.capacity();
        let n;

        unsafe {
            buf.set_len(cap);
            let result = retry(|| read(STDIN_FILENO, &mut buf[len..]));

            buf.set_len(len);

            n = result?;
            buf.set_len(len + n);
        }

        Ok(n)
    }

    fn find_bind(&self, ch: &Vec<u8>) -> Option<Keybind> {
        for &(ref bind, ref cmd) in &self.bindings {
            if bind == ch {
                return Some(cmd.clone());
            }
        }
        return None;
    }
}

fn settings_term() {
    let old_tio = tcgetattr(STDIN_FILENO).unwrap();
    let mut tio = old_tio;

    tio.input_flags.remove(
        InputFlags::INLCR | InputFlags::ICRNL,
    );
    tio.local_flags.remove(
        LocalFlags::ICANON | LocalFlags::ECHO,
    );
    tio.control_chars[SpecialCharacterIndices::VMIN as usize] = 0;
    tio.control_chars[SpecialCharacterIndices::VTIME as usize] = 0;

    tcsetattr(STDIN_FILENO, SetArg::TCSANOW, &tio).unwrap();
}

fn retry<F, R>(mut f: F) -> nix::Result<R>
where
    F: FnMut() -> nix::Result<R>,
{
    loop {
        match f() {                                                                                                                                                                                             
            Err(_e) => (),                                                                                                                                         
            res => return res,                                                                                                                                                                                   
        }
    }
}

fn wait_input() -> bool {
    let mut r_fds = FdSet::new();
    r_fds.insert(STDIN_FILENO);

    let mut e_fds = FdSet::new();
    r_fds.insert(STDIN_FILENO);

    loop {
        match select(
            STDIN_FILENO + 1,
            Some(&mut r_fds),
            None,
            Some(&mut e_fds),
            None.as_mut(),
        ) {
            Ok(n) => return n == 1,
            Err(_e) => return false,
        }
    }
}

#[derive(Clone)]
enum Keybind {
    Enter,
    Delete,
    CtrlL,
    ForwardChar,
    BackwardChar,
    PreviousHistory,
    NextHistory,
    // TODO
    Something,
}

fn bindings() -> Vec<(Cow<'static, [u8]>, Keybind)> {
    vec![
        (Cow::Borrowed(b"\r"      ), Keybind::Enter),           // Enter
        (Cow::Borrowed(b"\x7f"    ), Keybind::Delete),          // BackSpace
        (Cow::Borrowed(b"\x1b[A"  ), Keybind::PreviousHistory), // Up
        (Cow::Borrowed(b"\x1b[B"  ), Keybind::NextHistory),     // Down
        (Cow::Borrowed(b"\x1b[C"  ), Keybind::ForwardChar),     // Left
        (Cow::Borrowed(b"\x1b[D"  ), Keybind::BackwardChar),    // Right

        (Cow::Borrowed(b"\x01"    ), Keybind::Something),       // Ctrl-A
        (Cow::Borrowed(b"\x02"    ), Keybind::BackwardChar),    // Ctrl-B
        (Cow::Borrowed(b"\x05"    ), Keybind::Something),       // Ctrl-E
        (Cow::Borrowed(b"\x06"    ), Keybind::ForwardChar),     // Ctrl-F
        (Cow::Borrowed(b"\x07"    ), Keybind::Something),       // Ctrl-G
        (Cow::Borrowed(b"\x0a"    ), Keybind::Enter),           // Ctrl-J
        (Cow::Borrowed(b"\x0b"    ), Keybind::Something),       // Ctrl-K
        (Cow::Borrowed(b"\x0c"    ), Keybind::CtrlL),           // Ctrl-L
        (Cow::Borrowed(b"\x0d"    ), Keybind::Enter),           // Ctrl-N
        (Cow::Borrowed(b"\x0e"    ), Keybind::NextHistory),     // Ctrl-N
        (Cow::Borrowed(b"\x10"    ), Keybind::PreviousHistory), // Ctrl-P
        (Cow::Borrowed(b"\x12"    ), Keybind::Something),       // Ctrl-R
        (Cow::Borrowed(b"\x14"    ), Keybind::Something),       // Ctrl-T
        (Cow::Borrowed(b"\x19"    ), Keybind::Something),       // Ctrl-Y
        (Cow::Borrowed(b"\x1d"    ), Keybind::Something),       // Ctrl-]
        (Cow::Borrowed(b"\x1b\x08"), Keybind::Something),       // Escape, Ctrl-H
        (Cow::Borrowed(b"\x1b\x1d"), Keybind::Something),       // Escape, Ctrl-]
        (Cow::Borrowed(b"\x1b\x7f"), Keybind::Something),       // Escape, Rubout
        (Cow::Borrowed(b"\x1bb"   ), Keybind::Something),       // Escape, b
        (Cow::Borrowed(b"\x1bd"   ), Keybind::Something),       // Escape, d
        (Cow::Borrowed(b"\x1bf"   ), Keybind::Something),       // Escape, f
        (Cow::Borrowed(b"\x1bt"   ), Keybind::Something),       // Escape, t
        (Cow::Borrowed(b"\x1by"   ), Keybind::Something),       // Escape, y
        (Cow::Borrowed(b"\x1b#"   ), Keybind::Something),       // Escape, #
        (Cow::Borrowed(b"\x1b<"   ), Keybind::Something),       // Escape, <
        (Cow::Borrowed(b"\x1b>"   ), Keybind::Something),       // Escape, >
    ]
}
