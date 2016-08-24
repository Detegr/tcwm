extern crate ncurses;
extern crate uuid;

use ncurses::*;

use std::cell::{Cell, RefCell};
use std::fmt;
use std::ops::{Deref,DerefMut};
use std::rc::Rc;
use std::sync::{Once, ONCE_INIT};

pub type Id = uuid::Uuid;

static INIT: Once = ONCE_INIT;
static mut ROOT_CONTAINER: Option<*mut WindowContainer> = None;

#[allow(dead_code)]
fn log<T: Into<String>>(s: T) {
    use ::std::io::Write;
    let mut file = ::std::fs::OpenOptions::new().write(true).create(false).append(true).open("out.log").unwrap();
    let _ = file.write(s.into().as_bytes());
    let _ = file.write(b"\n");
}

pub struct Tcwm;
impl Tcwm {
    pub fn new() -> Result<Tcwm, CursesError> {
        let mut ret = Err(CursesError::CursesAlreadyInitialized);
        INIT.call_once(|| {
::std::fs::File::create("out.log").unwrap();
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

            let root = Box::new(WindowContainer::new());
            unsafe {
                ROOT_CONTAINER = Some(Box::into_raw(root));
            }
            ret = Ok(Tcwm)
        });
        ret
    }
}
impl Deref for Tcwm {
    type Target = WindowContainer;
    fn deref(&self) -> &Self::Target {
        unsafe {
            &*ROOT_CONTAINER.unwrap()
        }
    }
}
impl DerefMut for Tcwm {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            &mut *ROOT_CONTAINER.unwrap()
        }
    }
}
impl Drop for Tcwm {
    fn drop(&mut self) {
        endwin();
        unsafe {
            let root = Box::from_raw(ROOT_CONTAINER.unwrap());
            drop(root);
        }
    }
}


pub type WindowRef = Rc<RefCell<Window>>;
pub type ContainerRef = Rc<RefCell<WindowContainer>>;
pub const RESIZE: i32 = ncurses::KEY_RESIZE;

pub struct WindowContainer {
    id: Id,
    payload: Vec<WindowPayload>,
    direction: WindowSplitDirection,
    container_x: i32,
    container_y: i32,
    width: i32,
    height: i32,
    focus: usize,
    root: bool, // TODO: This is an ugly hack
}
impl PartialEq for WindowContainer {
    fn eq(&self, rhs: &WindowContainer) -> bool {
        self.id == rhs.id
    }
}
impl Eq for WindowContainer {}
impl fmt::Debug for WindowContainer {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fn format(this: &WindowContainer, fmt: &mut fmt::Formatter, indent: usize) -> fmt::Result {
            let this_indent = ::std::iter::repeat(" ").take(indent).collect::<String>();
            let others_indent = ::std::iter::repeat(" ").take(indent+2).collect::<String>();
            try!(write!(fmt, "{}{}{:?} ({})]\n", this_indent, "[C ", this.direction, this.id));
            for pl in this.payload.iter() {
                if pl.is_container() {
                    let pl = pl.as_container();
                    try!(format(&*pl.borrow(), fmt, indent + 4));
                } else {
                    let pl = pl.as_window();
                    let pl = pl.borrow();
                    try!(write!(fmt, "{}{} ({})]\n", others_indent, "[W", pl.id));
                };
            }
            Ok(())
        }
        format(self, fmt, 0)
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}
enum WindowPayload {
    Window(WindowRef),
    Container(ContainerRef),
}
impl WindowPayload {
    fn is_container(&self) -> bool {
        match *self {
            WindowPayload::Window(_) => false,
            WindowPayload::Container(_) => true,
        }
    }
    fn is_window(&self) -> bool {
        !self.is_container()
    }
    fn as_container(&self) -> ContainerRef {
        match *self {
            WindowPayload::Container(ref c) => c.clone(),
            _ => panic!("Not a container"),
        }
    }
    fn as_window(&self) -> WindowRef {
        match *self {
            WindowPayload::Window(ref wr) => wr.clone(),
            _ => panic!("Not a window"),
        }
    }
}
impl WindowContainer {
    fn new() -> WindowContainer {
        WindowContainer::new_container(0, 0, None)
    }
    fn new_container(x: i32, y: i32, window: Option<WindowRef>) -> WindowContainer {
        let is_root = window.is_none();
        let win = window.unwrap_or(Rc::new(RefCell::new(Window::new())));
        let (width, height) = {
            let win = win.borrow();
            (win.xmax, win.ymax)
        };
        let mut root = WindowContainer {
            id: uuid::Uuid::new_v4(),
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
        let _ = self.change_focus_internal(direction);
        self.refresh_windows(false);
    }
    fn change_focus_internal(&mut self, direction: Direction) -> Result<(), ()> {
        if self.payload[self.focus].is_container() {
            let c = self.payload[self.focus].as_container();
            return match c.borrow_mut().change_focus_internal(direction) {
                Err(_) => {
                    self.do_focus_change(direction)
                }
                Ok(_) => Ok(())
            };
        } else {
            self.do_focus_change(direction)
        }
    }
    pub fn delete(&mut self) {
        log(format!("{:?}", self));
        if self.id == unsafe { (*ROOT_CONTAINER.unwrap()).id } && self.payload.len() == 1 {
            // Only window, cannot delete
            return
        }
        let (window_deleted, delete_container) = self.with_focused_container_mut(|f| {
            let fwid = {
                let f = f.focused_window();
                let f = f.borrow();
                f.id
            };
            let fw_pos = f.payload.iter().position(|w| {
                if w.is_window() {
                    let w = w.as_window();
                    let w = w.borrow();
                    return w.id == fwid
                } else { false }
            });
            match fw_pos {
                Some(pos) => {
                    log("deleting window");
                    f.payload.remove(pos);
                    if f.focus > 0 {
                        f.focus -= 1;
                    }
                    let delete_container = {
                        if f.payload.len() == 0 {
                            log(format!("marking {} to be deleted", f.id));
                            Some(f.id)
                        } else {
                            None
                        }
                    };
                    (true, delete_container)
                }
                None => (false, None),
            }
        });
        if delete_container.is_some() {
            let cont_id = delete_container.unwrap();
            if let Some(container) = self.find(cont_id) {
                log(format!("Searching for parent of {} starting from {}", cont_id, self.id));
                self.with_parent_of(&*container.borrow(), |p| {
                    log(format!("Parent {} found", p.id));
                    log(format!("Parent's payload is {}", p.payload.len()));
                    let pos = p.payload.iter().position(|w| {
                        if w.is_container() {
                            let c = w.as_container();
                            let cid = c.borrow().id;
                            log(format!("{} == {}", cid, cont_id));
                            return cid == cont_id
                        }
                        false
                    });
                    log(format!("{:?}", pos));
                    if let Some(pos) = pos {
                        log("deleting container");
                        p.payload.remove(pos);
                        if p.focus > 0 {
                            p.focus -= 1;
                        }
                    }
                });
            }
        }
        if window_deleted {
            log("window deleted, resizing");
            //self.on_resize(true, (0,0,0,0));
            //self.refresh_windows(true);
            WindowContainer::resize();
        }
    }
    fn do_focus_change(&mut self, direction: Direction) -> Result<(), ()> {
        match direction {
            Direction::Left | Direction::Up => {
                if self.direction.direction_ok(direction) {
                    if self.focus == 0 {
                        Err(())
                    } else {
                        self.focus -= 1;
                        Ok(())
                    }
                } else { Err(()) }
            }
            Direction::Right | Direction::Down => {
                if self.direction.direction_ok(direction) {
                    if self.focus == self.payload.len() - 1 {
                        Err(())
                    } else {
                        self.focus += 1;
                        Ok(())
                    }
                } else { Err(()) }
            }
        }
    }
    fn find(&self, id: Id) -> Option<ContainerRef> {
        for pl in self.payload.iter() {
            if pl.is_container() {
                let cpl = pl.as_container();
                let pl = cpl.borrow();
                let plid = pl.id;
                if plid == id { return Some(cpl.clone()) }
                else { return pl.find(id) }
            }
        }
        None
    }
    pub fn print(&mut self, s: &str) {
        self.with_focused_container_mut(|f| {
            let f = f.focused_window();
            let mut f = f.borrow_mut();
            f.print(s);
        })
    }
    pub fn print_overwriting(&mut self, s: &str) {
        self.with_focused_container_mut(|f| {
            let f = f.focused_window();
            let mut f = f.borrow_mut();
            f.print_overwriting(s);
        })
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
            WindowPayload::Window(win_payload) => {
                let (x, y) = {
                    let win = win_payload.borrow();
                    (win.x, win.y)
                };
                let mut new = WindowContainer::new_container(x, y, Some(win_payload));
                new.direction = direction;
                self.payload.insert(self.focus, WindowPayload::Container(Rc::new(RefCell::new(new))));
            }
            _ => unreachable!()
        }
    }
    pub fn split(&mut self) {
        log(format!("{:?}", self));
        self.with_focused_container_mut(|f| {
            let win = match f.direction {
                WindowSplitDirection::Vertical => f.split_vertical(),
                WindowSplitDirection::Horizontal => f.split_horizontal(),
            };
            f.focus += 1;
            f.payload.push(WindowPayload::Window(Rc::new(RefCell::new(win))));
        });
        WindowContainer::resize();
    }
    pub fn with_focused_container_mut<F, T>(&mut self, f: F) -> T
        where F: Fn(&mut WindowContainer) -> T
    {
        match self.focused_container() {
            Some(w) => f(&mut *w.borrow_mut()),
            None => f(self),
        }
    }
    pub fn with_focused_container<F, T>(&self, f: F) -> T
        where F: Fn(&WindowContainer) -> T
    {
        match self.focused_container() {
            Some(w) => f(&*w.borrow()),
            None => f(self),
        }
    }
    fn with_parent_of<F, T>(&self, c: &WindowContainer, f: F) -> T
        where F: Fn(&mut WindowContainer) -> T
    {
        fn find_first_parent(from: &WindowContainer, c: &WindowContainer) -> Option<ContainerRef> {
            for pl in from.payload.iter().filter(|pl| pl.is_container()).map(|pl| pl.as_container()) {
                if *c == *pl.borrow() {
                    return None
                } else {
                    find_parent(pl.clone(), c);
                }
            }
            unreachable!();
        }
        fn find_parent(from: ContainerRef, c: &WindowContainer) -> Option<ContainerRef> {
            for pl in from.borrow().payload.iter().filter(|pl| pl.is_container()).map(|pl| pl.as_container()) {
                if *c == *pl.borrow() {
                    return Some(from.clone())
                } else {
                    find_parent(pl.clone(), c);
                }
            }
            unreachable!();
        }
        match find_first_parent(self, c) {
            Some(parent) => {
                f(&mut *parent.borrow_mut())
            }
            None => {
                unsafe {
                    let ref mut rc = *ROOT_CONTAINER.unwrap();
                    f(rc)
                }
            }
        }
    }
    fn focused_window(&self) -> WindowRef {
        self.payload[self.focus].as_window()
    }
    fn focused_container(&self) -> Option<ContainerRef> {
        let ref win = self.payload[self.focus];
        match win {
            &WindowPayload::Container(ref container) => {
                let c = container.borrow();
                if c.payload[c.focus].is_container() {
                    c.focused_container()
                } else {
                    Some(container.clone()) }
            },
            &WindowPayload::Window(_) => None
        }
    }
    fn calculate_dimensions(&self, window_count: Option<i32>, direction: Option<WindowSplitDirection>) -> (i32, i32, i32) {
        // TODO: Not sure if this function makes any sense
        let window_count = window_count.unwrap_or(self.payload.len() as i32);
        let (dim, pos) = match direction.unwrap_or(self.direction) {
            WindowSplitDirection::Horizontal => (self.height, self.container_y),
            WindowSplitDirection::Vertical => (self.width, self.container_x),
        };
        let size = dim / window_count;
        let rounding_error = dim % size;
        (pos, size, rounding_error)
    }
    fn refresh_dimensions(&mut self, dimensions: (i32, i32, i32)) {
        let (pos, size, _) = dimensions;
        for (i, window) in self.payload.iter_mut().enumerate() {
            match window {
                &mut WindowPayload::Window(ref w) => {
                    let mut w = w.borrow_mut();
                    w.cursor.set((0, 0));
                    match self.direction {
                        WindowSplitDirection::Horizontal => {
                            w.y = pos + (i as i32) * size;
                            w.ymax = size;
                        }
                        WindowSplitDirection::Vertical => {
                            w.x = pos + (i as i32) * size;
                            w.xmax = size;
                        }
                    }
                    WindowContainer::reresize_window(&mut *w);
                }
                _ => {}
            }
        }
    }
    fn split_vertical(&mut self) -> Window {
        let dim = self.calculate_dimensions(Some(self.payload.len() as i32 + 1), None);
        self.refresh_dimensions(dim);
        let (_, window_width, rounding_error) = dim;
        let new_window_x = self.container_x + (self.payload.len() as i32 * window_width);
        Window::new_window(new_window_x, self.container_y, (window_width + rounding_error, self.height), true)
    }
    fn split_horizontal(&mut self) -> Window {
        let dim = self.calculate_dimensions(Some(self.payload.len() as i32 + 1), None);
        self.refresh_dimensions(dim);
        let (_, window_height, rounding_error) = dim;
        let new_window_y = self.container_y + (self.payload.len() as i32 * window_height);
        Window::new_window(self.container_x, new_window_y, (self.width, window_height + rounding_error), self.container_x > 0)
    }
    fn reresize_window(w: &mut Window) {
        wclear(w.win);
        if let Some(bwin) = w.border_win {
            wresize(w.win, w.ymax - 1, w.xmax - 1);
            wresize(w.header_win, 1, w.xmax);
            wresize(bwin, w.ymax - 1, 1);
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
        self.refresh_windows_internal(reprint, true)
    }
    fn refresh_windows_internal(&mut self, reprint: bool, in_focus_chain: bool) {
        for (i, window) in self.payload.iter_mut().enumerate() {
            match window {
                &mut WindowPayload::Window(ref w) => {
                    let mut w = w.borrow_mut();
                    let header_color = {
                        let focused = in_focus_chain && self.focus == i;
                        let color = if focused { Color::StatusSelected } else { Color::Status };
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
                        w.cursor.set((0, 0));
                        w.reprint_buffer();
                    } else {
                        wrefresh(w.win);
                    }
                }
                &mut WindowPayload::Container(ref c) => {
                    let mut c = c.borrow_mut();
                    let in_focus_chain = in_focus_chain && self.focus == i;
                    c.refresh_windows_internal(reprint, in_focus_chain);
                }
            }
        }
    }
    pub fn set_header(&mut self, header: &str) {
        self.with_focused_container_mut(|w| {
            let w = w.payload[w.focus].as_window();
            let mut w = w.borrow_mut();
            w.header = header.to_owned();
            w.print_header();
        })
    }
    fn on_resize(&mut self, first: bool, mut parent: (i32, i32, i32, i32)) {
        fn window_filter(p: &mut WindowPayload) -> Option<WindowRef> {
            if p.is_window() {
                Some(p.as_window())
            } else {
                None
            }
        }
        fn container_filter(p: &mut WindowPayload) -> Option<ContainerRef> {
            if p.is_container() {
                Some(p.as_container())
            } else {
                None
            }
        }

        if first {
            // One more ugly hack
            let mut h = 0;
            let mut w = 0;
            getmaxyx(stdscr, &mut h, &mut w);
            self.height = h;
            self.width = w;
            parent = (self.width, self.height, 0, 0);
        }
        let windows_len = self.payload.iter_mut()
                                      .filter_map(window_filter)
                                      .size_hint().1.unwrap();

        let containers_len = self.payload.iter_mut()
                                         .filter_map(container_filter)
                                         .size_hint().1.unwrap();
        let (cpos_h, csize_h, crounding_error_h) = self.calculate_dimensions(Some(containers_len as i32), Some(WindowSplitDirection::Horizontal));
        let (cpos_v, csize_v, crounding_error_v) = self.calculate_dimensions(Some(containers_len as i32), Some(WindowSplitDirection::Vertical));
        let (wpos_h, wsize_h, wrounding_error_h) = self.calculate_dimensions(Some(windows_len as i32), Some(WindowSplitDirection::Horizontal));
        let (wpos_v, wsize_v, wrounding_error_v) = self.calculate_dimensions(Some(windows_len as i32), Some(WindowSplitDirection::Vertical));

        for (i, pl) in self.payload.iter_mut().enumerate() {
            if pl.is_container() {
                let pl = pl.as_container();
                let mut container = pl.borrow_mut();
                match self.direction {
                    WindowSplitDirection::Horizontal => {
                        let newh = csize_h + if i == (containers_len - 1) { crounding_error_h } else { 0 };
                        let newy = cpos_h + (i * csize_h as usize) as i32;
                        container.container_x = parent.2;
                        container.width = parent.0;
                        container.container_y = newy;
                        container.height = newh;
                    },
                    WindowSplitDirection::Vertical => {
                        let neww = csize_v + if i == (containers_len - 1) { crounding_error_v } else { 0 };
                        let newx = cpos_v + (i * csize_v as usize) as i32;
                        container.height = parent.1;
                        container.container_y = parent.3;
                        container.container_x = newx;
                        container.width = neww;
                    }
                }
                let p = (container.width, container.height, container.container_x, container.container_y);
                container.on_resize(false, p);
            } else {
                let win = pl.as_window();
                let mut win = win.borrow_mut();
                match self.direction {
                    WindowSplitDirection::Horizontal => {
                        let newy = wpos_h + (i * wsize_h as usize) as i32;
                        let newymax = wsize_h + if i == (windows_len - 1) { wrounding_error_h } else { 0 };
                        win.y = newy;
                        win.ymax = newymax;
                        win.x = self.container_x;
                        win.xmax = self.width;
                    },
                    WindowSplitDirection::Vertical => {
                        let newx = wpos_v + (i * wsize_v as usize) as i32;
                        let newxmax = wsize_v + if i == (windows_len - 1) { wrounding_error_v } else { 0 };
                        win.x = newx;
                        win.xmax = newxmax;
                        win.y = self.container_y;
                        win.ymax = self.height;
                    }
                }
                WindowContainer::reresize_window(&mut *win);
            }
        }
    }
    pub fn wait_for_key(&self) -> i32 {
        let resize_needed = ::std::cell::Cell::new(false);
        let ret = self.with_focused_container(|f| {
            let w = f.focused_window();
            let ret = ncurses::wgetch(w.borrow().win);
            if ret == RESIZE {
                resize_needed.set(true);
            }
            ret
        });
        if resize_needed.get() {
            WindowContainer::resize();
        }
        ret
    }
    pub fn resize() {
        unsafe {
            let ref mut rc = *ROOT_CONTAINER.unwrap();
            rc.on_resize(true, (0, 0, 0, 0));
            rc.refresh_windows(true);
        }
    }
}

pub enum CursesError {
    CursesAlreadyInitialized,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum WindowSplitDirection {
    Horizontal,
    Vertical,
}
impl WindowSplitDirection {
    fn direction_ok(&self, dir: Direction) -> bool {
        match *self {
            WindowSplitDirection::Vertical => {
                dir == Direction::Left
                || dir == Direction::Right
            }
            WindowSplitDirection::Horizontal => {
                dir == Direction::Up
                || dir == Direction::Down
            }
        }
    }
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

pub struct Window {
    id: Id,
    win: WINDOW,
    border_win: Option<WINDOW>,
    header_win: WINDOW,
    x: i32,
    y: i32,
    xmax: i32,
    ymax: i32,
    cursor: Cell<(i32, i32)>,
    lines: Vec<String>,
    header: String,
}
impl PartialEq for Window {
    fn eq(&self, rhs: &Window) -> bool {
        self.id == rhs.id
    }
}
impl Eq for Window {}
impl Window {
    fn new_window(x: i32, y: i32, dimensions: (i32, i32), border: bool) -> Window {
        let (xmax, ymax) = dimensions;
        let bwin = if border { Some(newwin(ymax, 1, y, x)) } else { None };
        let win = if border { newwin(ymax-1, xmax, y, x+1) } else { newwin(ymax-1, xmax, y, x) };
        let hwin = newwin(1, xmax, y + ymax - 1, x);
        wbkgd(hwin, COLOR_PAIR(Color::StatusSelected.into()));
        nodelay(win, true);
        Window {
            id: uuid::Uuid::new_v4(),
            win: win,
            border_win: bwin,
            header_win: hwin,
            x: x,
            y: y,
            xmax: xmax,
            ymax: ymax,
            cursor: Cell::new((0, 0)),
            lines: vec![],
            header: "New window".into(),
        }
    }
    fn new() -> Window {
        let mut xmax = 0;
        let mut ymax = 0;
        getmaxyx(stdscr, &mut ymax, &mut xmax);
        Window::new_window(0, 0, (xmax, ymax), false)
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
        let (x, mut y) = self.cursor.get();
        if y >= self.ymax {
            // TODO: Scroll
            return;
        }
        mvwprintw(self.win, y, x, s);
        y += (s.len() as i32 / (self.xmax - 1)) + 1;
        self.cursor.set((x, y));
    }
    pub fn print(&mut self, s: &str) {
        self.print_internal(s);
        self.lines.push(s.into());
        wrefresh(self.win);
    }
    pub fn print_overwriting(&mut self, s: &str) {
        let (_, y) = self.cursor.get();
        self.cursor.set((0, y));
        wmove(self.win, y, 0);
        wclrtoeol(self.win);

        // TODO: Scroll?
        wprintw(self.win, s);

        self.lines.pop();
        self.lines.push(s.into());
        wrefresh(self.win);
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        delwin(self.win);
        if let Some(bwin) = self.border_win {
            delwin(bwin);
        }
    }
}
