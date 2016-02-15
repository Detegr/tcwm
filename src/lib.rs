extern crate ncurses;

use ncurses::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::ops::{Deref, DerefMut};

type ContainerRef = Rc<RefCell<WindowContainer>>;

pub struct WindowContainer {
    payload: Vec<WindowPayload>,
    direction: WindowSplitDirection,
    container_x: i32,
    container_y: i32,
    width: i32,
    height: i32,
    focus: usize,
    root: bool, // TODO: This is an ugly hack
}
#[derive(Copy, Clone, PartialEq)]
pub enum WindowSplitDirection {
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
    Window(Curses),
    Container(ContainerRef),
}
impl WindowPayload {
    fn is_container(&self) -> bool {
        match *self {
            WindowPayload::Window(_) => false,
            WindowPayload::Container(_) => true,
        }
    }
    fn as_window(&self) -> &Curses {
        match *self {
            WindowPayload::Window(ref wr) => wr,
            _ => panic!("Not a window"),
        }
    }
    fn as_window_mut(&mut self) -> &mut Curses {
        match *self {
            WindowPayload::Window(ref mut wr) => wr,
            _ => panic!("Not a window"),
        }
    }
}
impl WindowContainer {
    pub fn new() -> WindowContainer {
        WindowContainer::new_container(0, 0, None)
    }
    fn new_container(x: i32, y: i32, window: Option<Curses>) -> WindowContainer {
        let is_root = window.is_none();
        let win = window.unwrap_or(Curses::new());
        let width = win.xmax;
        let height = win.ymax;
        let mut root = WindowContainer {
            payload: vec![WindowPayload::Window(win)],
            direction: WindowSplitDirection::Vertical,
            container_x: x,
            container_y: y,
            width: width,
            height: height,
            focus: 0,
            root: is_root,
        };
        root.refresh_windows(false);
        root
    }
    pub fn change_focus(&mut self, direction: Direction) {
        match direction {
            Direction::Left => {
                if self.focus == 0 {
                    self.focus = self.payload.len() - 1;
                } else {
                    self.focus -= 1;
                }
            }
            Direction::Right => {
                if self.focus == self.payload.len() - 1 {
                    self.focus = 0;
                } else {
                    self.focus += 1;
                }
            }
            _ => panic!("Direction NYI")
        }
        self.refresh_windows(true);
    }
    pub fn print(&mut self, s: &str) {
        self.with_focused_container(|f| f.focused_window().print(s))
    }
    pub fn set_split_direction(&mut self, direction: WindowSplitDirection) {
        if self.payload.len() == 1 {
            self.direction = direction;
            return
        }
        if let WindowPayload::Container(ref container) = self.payload[self.focus] {
            container.borrow_mut().set_split_direction(direction);
            return;
        }
        let win = self.payload.remove(self.focus);
        match win {
            WindowPayload::Window(win) => {
                let x = win.x;
                let y = win.y;
                let mut new = WindowContainer::new_container(x, y, Some(win));
                new.direction = direction;
                self.payload.insert(self.focus, WindowPayload::Container(Rc::new(RefCell::new(new))));
            }
            _ => unreachable!()
        }
    }
    pub fn split(&mut self) {
        self.with_focused_container(|f| {
            let direction = f.direction;
            f.do_split(direction);
        });
        self.refresh_windows(false);
    }
    fn do_split(&mut self, direction: WindowSplitDirection) {
        match direction {
            WindowSplitDirection::Vertical => self.split_vertical(),
            WindowSplitDirection::Horizontal => self.split_horizontal(),
        }
    }
    fn with_focused_container<F>(&mut self, f: F)
        where F: Fn(&mut WindowContainer)
    {
        match self.focused_container() {
            Some(w) => f(&mut *w.borrow_mut()),
            None => f(self),
        }
    }
    fn focused_window(&mut self) -> &mut Curses {
        self.payload[self.focus].as_window_mut()
    }
    fn focused_container(&self) -> Option<ContainerRef> {
        let ref win = self.payload[self.focus];
        match win {
            &WindowPayload::Container(ref container) => {
                let c = container.borrow();
                if c.payload[c.focus].is_container() {
                    c.focused_container()
                } else {
                    Some(container.clone())
                }
            },
            &WindowPayload::Window(_) => None
        }
    }
    fn split_vertical(&mut self) {
        self.with_focused_container(WindowContainer::do_split_vertical)
    }
    fn do_split_vertical(&mut self) {
        let window_count = self.payload.len() as i32 + 1;
        let window_width = self.width / window_count;
        let rounding_error = self.width % window_width;
        if window_width < 20 {
            return;
        }
        for (i, window) in self.payload.iter_mut().enumerate() {
            match window {
                &mut WindowPayload::Window(ref mut w) => {
                    //let mut w = w.borrow_mut();
                    w.cursor = RefCell::new((0, 0));
                    w.xmax = window_width;
                    w.x = self.container_x + (i as i32) * window_width;
                    WindowContainer::reresize_window(&mut *w);
                }
                _ => {}
            }
        }
        let new_window_x = self.container_x + (self.payload.len() as i32 * window_width);
        let split = Curses::new_window(new_window_x, self.container_y, (window_width + rounding_error, self.height), true);
        self.focus += 1;
        self.payload.push(WindowPayload::Window(split));
    }
    fn split_horizontal(&mut self) {
        self.with_focused_container(WindowContainer::do_split_horizontal)
    }
    fn do_split_horizontal(&mut self) {
        let window_count = self.payload.len() as i32 + 1;
        let window_height = self.height / window_count;
        let rounding_error = self.height % window_height;
        if window_height < 5 {
            return;
        }
        for (i, window) in self.payload.iter_mut().enumerate() {
            match window {
                &mut WindowPayload::Window(ref mut w) => {
                    //let mut w = w.borrow_mut();
                    w.cursor = RefCell::new((0, 0));
                    w.ymax = window_height;
                    w.y = self.container_y + (i as i32) * window_height;
                    WindowContainer::reresize_window(&mut *w);
                }
                _ => {}
            }
        }
        let new_window_y = self.container_y + (self.payload.len() as i32 * window_height);
        let split = Curses::new_window(self.container_x, new_window_y, (self.width, window_height + rounding_error), self.container_x > 0);
        self.focus += 1;
        self.payload.push(WindowPayload::Window(split));
    }
    fn reresize_window(w: &mut Curses) {
        wclear(w.win);
        if let Some(bwin) = w.border_win {
            wresize(w.win, w.ymax - 1, w.xmax - 1);
            wresize(w.header_win, 1, w.xmax);
            mvwin(bwin, w.y, w.x);
            mvwin(w.header_win, w.y + w.ymax - 1, w.x);
            mvwin(w.win, w.y, w.x + 1);
        } else {
            wresize(w.win, w.ymax - 1, w.xmax);
            wresize(w.header_win, 1, w.xmax);
            mvwin(w.header_win, w.y + w.ymax - 1, w.x);
            mvwin(w.win, w.y, w.x);
        }
    }
    fn refresh_windows(&mut self, reprint: bool) {
        for (i, window) in self.payload.iter_mut().enumerate() {
            match window {
                &mut WindowPayload::Window(ref mut w) => {
                    let header_color = {
                        let focused = self.focus == i;
                        let color = if focused { Color::StatusSelected } else { Color::Status.into() };
                        COLOR_PAIR(color.into())
                    };
                    wbkgd(w.header_win, header_color);
                    if self.root {
                        w.header = format!("Window {} ({}, {}) ({}, {})", i, w.x, w.y, w.xmax, w.ymax);
                    } else {
                        w.header = format!("Container {} ({}, {}) ({}, {})", i, w.x, w.y, w.xmax, w.ymax);
                    }
                    w.print_header();
                    if let Some(bwin) = w.border_win {
                        mvwvline(bwin, 0, 0, ACS_HLINE(), 1000);
                        wrefresh(bwin);
                    }
                    if reprint {
                        w.reprint_buffer();
                    } else {
                        wrefresh(w.win);
                    }
                }
                &mut WindowPayload::Container(ref c) => {
                    let mut c = c.borrow_mut();
                    c.refresh_windows(reprint);
                }
            }
        }
    }
    pub fn set_header(&mut self, header: &str) {
        self.with_focused_container(|w| {
            let mut w = w.payload[w.focus].as_window_mut();
            w.header = header.to_owned();
            w.print_header();
        })
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

struct Curses {
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
    fn new_window(x: i32, y: i32, dimensions: (i32, i32), border: bool) -> Curses {
        let (xmax, ymax) = dimensions;
        let bwin = if border { Some(newwin(ymax, 1, y, x)) } else { None };
		let win = if border { newwin(ymax-1, xmax, y, x+1) } else { newwin(ymax-1, xmax, y, x) };
        let hwin = newwin(1, xmax, y + ymax - 1, x);
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
