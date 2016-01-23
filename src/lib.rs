extern crate ncurses;
extern crate rctree;

use ncurses::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::collections::HashMap;

pub type WindowRef = RefCell<Curses>;

pub struct WindowContainer {
    payload: Vec<WindowPayload>,
    direction: WindowSplitDirection,
    width: i32,
    height: i32,
    focus: Option<usize>,
    root: bool, // TODO: This is an ugly hack
}
enum WindowSplitDirection {
    Horizontal,
    Vertical,
}
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}
enum WindowPayload {
    Window(WindowRef),
    Container(WindowContainer),
}
impl WindowContainer {
    pub fn new() -> WindowContainer {
        let root_window = Curses::new();
        let width = root_window.xmax;
        let height = root_window.ymax;
        let mut root = WindowContainer {
            payload: vec![WindowPayload::Window(RefCell::new(root_window))],
            direction: WindowSplitDirection::Vertical,
            width: width,
            height: height,
            focus: Some(0),
            root: true,
        };
        root.refresh_windows(false);
        root
    }
    pub fn change_focus(&mut self, direction: Direction) {
        match direction {
            Direction::Left => {
                if let Some(focus) = self.focus {
                    if focus == 0 {
                        self.focus = Some(self.payload.len() - 1);
                    } else {
                        self.focus = Some(focus - 1);
                    }
                }
            }
            Direction::Right => {
                if let Some(focus) = self.focus {
                    if focus == self.payload.len() - 1 {
                        self.focus = Some(0);
                    } else {
                        self.focus = Some(focus + 1);
                    }
                }
            }
            _ => panic!("Direction NYI")
        }
        self.refresh_windows(false);
    }
    pub fn print(&mut self, s: &str) {
        if let Some(focus) = self.focus {
            if let WindowPayload::Window(ref w) = self.payload[focus] {
                let mut w = w.borrow_mut();
                w.print(s);
            }
        }
    }
    pub fn split(&mut self) {
        let window_count = self.payload.len() as i32 + 1;
        let window_width = self.width / window_count;
        if window_width < 20 {
            return;
        }
        for (i, window) in self.payload.iter().enumerate() {
            match window {
                &WindowPayload::Window(ref w) => {
                    let mut w = w.borrow_mut();
                    w.cursor = RefCell::new((0, 0));
                    w.xmax = window_width;
                    w.x = i as i32 * window_width;
                    wclear(w.win);
                    if let Some(bwin) = w.border_win {
                        wresize(w.win, w.ymax-1, w.xmax-1);
                        wresize(w.header_win, 1, w.xmax);
                        mvwin(bwin, w.y, w.x);
                        mvwin(w.header_win, w.ymax-1, w.x);
                        mvwin(w.win, w.y, w.x+1);
                    } else {
                        wresize(w.win, w.ymax-1, w.xmax);
                        wresize(w.header_win, 1, w.xmax);
                        mvwin(w.header_win, w.ymax-1, w.x);
                        mvwin(w.win, w.y, w.x);
                    }
                }
                _ => {}
            }
        }
        let new_window_x = self.payload.len() as i32 * window_width;
        let mut split = Curses::new_window(new_window_x, 0, (window_width, self.height), true);
        if let Some(focus_index) = self.focus {
            self.focus = Some(focus_index + 1)
        }
        self.payload.push(WindowPayload::Window(RefCell::new(split)));

        self.refresh_windows(true);
    }
    fn refresh_windows(&mut self, reprint: bool) {
        for (i, window) in self.payload.iter().enumerate() {
            match window {
                &WindowPayload::Window(ref w) => {
                    let mut w = w.borrow_mut();
                    let wx = w.x;

                    let header_color = {
                        let focused = self.focus.map_or(false, |fi| fi == i);
                        let color = if focused { Color::StatusSelected } else { Color::Status.into() };
                        COLOR_PAIR(color.into())
                    };
                    wbkgd(w.header_win, header_color);
                    w.print_header();
                    if let Some(bwin) = w.border_win {
                        mvwvline(bwin, 0, 0, ACS_HLINE(), 1000);
                        wrefresh(bwin);
                    }
                    if reprint {
                        w.reprint_buffer();
                    }
                }
                _ => {}
            }
        }
    }
}
impl Drop for WindowContainer {
    fn drop(&mut self) {
        if self.root {
            endwin();
        }
    }
}

pub enum CursesError {
    CursesAlreadyInitialized,
}

pub enum CursesSplitDirection {
    Horizontal,
    Vertical,
}

pub fn wait_for_key() -> i32 {
    ncurses::getch()
}

enum Color {
    Default = 1,
    Selection = 2,
    Status = 3,
    StatusSelected = 4,
}
impl Into<i16> for Color {
    fn into(self) -> i16 {
        self as i16
    }
}

pub struct Curses {
	win: WINDOW,
    border_win: Option<WINDOW>,
    header_win: WINDOW,
    x: i32,
    y: i32,
    xmax: i32,
    ymax: i32,
    cursor: RefCell<(i32, i32)>,
    lines: Vec<String>,
    header: String,
}
impl Curses {
    fn new_window(mut x: i32, mut y: i32, dimensions: (i32, i32), border: bool) -> Curses {
        let mut xmax = 0;
        let mut ymax = 0;
        let (xmax, ymax) = dimensions;
        let bwin = if border { Some(newwin(ymax, 1, y, x)) } else { None };
		let win = if border { newwin(ymax-1, xmax, y, x+1) } else { newwin(ymax-1, xmax, y, x) };
        let hwin = newwin(1, xmax, ymax-1, x);
        wbkgd(hwin, COLOR_PAIR(Color::StatusSelected.into()));
        Curses {
            win: win,
            border_win: bwin,
            header_win: hwin,
            x: x,
            y: y,
            xmax: xmax,
            ymax: ymax,
            cursor: RefCell::new((0, 0)),
            lines: vec![],
            header: "New window".into(),
        }
    }
    fn new() -> Curses {
        initscr();
        if !has_colors() {
            panic!("No colors");
        }
        curs_set(ncurses::CURSOR_VISIBILITY::CURSOR_INVISIBLE);
        noecho();
        refresh();
        start_color();

        init_pair(Color::Selection.into(), COLOR_GREEN, COLOR_BLACK);
        init_pair(Color::Status.into(), COLOR_WHITE, COLOR_BLUE);
        init_pair(Color::StatusSelected.into(), COLOR_BLACK, COLOR_CYAN);
        init_pair(Color::Default.into(), COLOR_GREEN, COLOR_BLACK);

        let mut xmax = 0;
        let mut ymax = 0;
        getmaxyx(stdscr, &mut ymax, &mut xmax);
        Curses::new_window(0, 0, (xmax, ymax), false)
    }
    pub fn getch(&self) -> i32 {
        wgetch(self.win)
    }
    fn reprint_buffer(&mut self) {
        for line in self.lines.iter() {
            self.print_internal(line);
        }
        wrefresh(self.win);
    }
    fn print_header(&mut self) {
        let margin = if self.border_win.is_some() { 2 } else { 1 };
        mvwprintw(self.header_win, 0, margin, &self.header[..]);
        wrefresh(self.header_win);
    }
    fn print_internal(&self, s: &str) {
        let (ref mut x, ref mut y) = *self.cursor.borrow_mut();
        if *y >= self.ymax {
            // TODO: Scroll
            return;
        }
        mvwprintw(self.win, *y, *x, s);
        *y += (s.len() as i32 / (self.xmax - 1)) + 1;
    }
    pub fn print(&mut self, s: &str) {
        self.print_internal(s);
        self.lines.push(s.into());
        wrefresh(self.win);
    }
}
impl Drop for Curses {
    fn drop(&mut self) {
        delwin(self.win);
        if let Some(bwin) = self.border_win {
            delwin(bwin);
        }
    }
}
