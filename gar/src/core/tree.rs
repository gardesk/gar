use x11rb::protocol::xproto::Window as XWindow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone)]
pub enum Node {
    Internal {
        split: SplitDirection,
        ratio: f32,
        left: Box<Node>,
        right: Box<Node>,
    },
    Leaf {
        window: Option<XWindow>,
    },
}

impl Default for Node {
    fn default() -> Self {
        Self::empty()
    }
}

impl Node {
    pub fn empty() -> Self {
        Node::Leaf { window: None }
    }

    pub fn with_window(window: XWindow) -> Self {
        Node::Leaf {
            window: Some(window),
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Node::Leaf { window: None })
    }

    pub fn window_count(&self) -> usize {
        match self {
            Node::Leaf { window: Some(_) } => 1,
            Node::Leaf { window: None } => 0,
            Node::Internal { left, right, .. } => left.window_count() + right.window_count(),
        }
    }

    pub fn windows(&self) -> Vec<XWindow> {
        match self {
            Node::Leaf { window: Some(w) } => vec![*w],
            Node::Leaf { window: None } => vec![],
            Node::Internal { left, right, .. } => {
                let mut windows = left.windows();
                windows.extend(right.windows());
                windows
            }
        }
    }

    pub fn contains(&self, window: XWindow) -> bool {
        match self {
            Node::Leaf { window: Some(w) } => *w == window,
            Node::Leaf { window: None } => false,
            Node::Internal { left, right, .. } => left.contains(window) || right.contains(window),
        }
    }

    /// Determine optimal split direction based on container dimensions.
    /// If wider than tall, split vertically (side-by-side).
    /// If taller than wide, split horizontally (stacked).
    pub fn smart_split_direction(rect: &Rect) -> SplitDirection {
        if rect.width > rect.height {
            SplitDirection::Vertical
        } else {
            SplitDirection::Horizontal
        }
    }

    /// Insert a window into the tree with smart splitting.
    /// If the tree is empty, the window becomes the root.
    /// Split direction is determined by container dimensions.
    pub fn insert(&mut self, new_window: XWindow, target: Option<XWindow>) {
        self.insert_with_rect(new_window, target, Rect::new(0, 0, 1920, 1080));
    }

    /// Insert with known container rect for smart split calculation.
    pub fn insert_with_rect(&mut self, new_window: XWindow, target: Option<XWindow>, rect: Rect) {
        match self {
            Node::Leaf { window: None } => {
                // Empty tree - just place the window here
                *self = Node::with_window(new_window);
            }
            Node::Leaf { window: Some(existing) } => {
                // Split this leaf using smart split direction
                let direction = Self::smart_split_direction(&rect);
                let existing_window = *existing;
                *self = Node::Internal {
                    split: direction,
                    ratio: 0.5,
                    left: Box::new(Node::with_window(existing_window)),
                    right: Box::new(Node::with_window(new_window)),
                };
            }
            Node::Internal { split, ratio, left, right } => {
                // Find the target window and insert next to it
                let (left_rect, right_rect) = rect.split(*split, *ratio);

                if let Some(target) = target {
                    if left.contains(target) {
                        left.insert_with_rect(new_window, Some(target), left_rect);
                    } else if right.contains(target) {
                        right.insert_with_rect(new_window, Some(target), right_rect);
                    } else {
                        // Target not found, insert in right subtree
                        right.insert_with_rect(new_window, None, right_rect);
                    }
                } else {
                    // No target specified, insert in right subtree
                    right.insert_with_rect(new_window, None, right_rect);
                }
            }
        }
    }

    /// Remove a window from the tree.
    /// Returns true if the window was found and removed.
    pub fn remove(&mut self, window: XWindow) -> bool {
        match self {
            Node::Leaf { window: Some(w) } if *w == window => {
                *self = Node::empty();
                true
            }
            Node::Leaf { .. } => false,
            Node::Internal { left, right, .. } => {
                if left.remove(window) {
                    // Left child removed the window, check if we need to collapse
                    if left.is_empty() {
                        // Promote right child
                        *self = std::mem::take(right.as_mut());
                    }
                    true
                } else if right.remove(window) {
                    // Right child removed the window, check if we need to collapse
                    if right.is_empty() {
                        // Promote left child
                        *self = std::mem::take(left.as_mut());
                    }
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Calculate geometries for all windows in the tree.
    pub fn calculate_geometries(&self, rect: Rect) -> Vec<(XWindow, Rect)> {
        match self {
            Node::Leaf { window: Some(w) } => vec![(*w, rect)],
            Node::Leaf { window: None } => vec![],
            Node::Internal {
                split,
                ratio,
                left,
                right,
            } => {
                let (left_rect, right_rect) = rect.split(*split, *ratio);
                let mut geometries = left.calculate_geometries(left_rect);
                geometries.extend(right.calculate_geometries(right_rect));
                geometries
            }
        }
    }

    /// Find the first window in the tree (leftmost leaf).
    pub fn first_window(&self) -> Option<XWindow> {
        match self {
            Node::Leaf { window } => *window,
            Node::Internal { left, right, .. } => {
                left.first_window().or_else(|| right.first_window())
            }
        }
    }

    /// Equalize all split ratios to 0.5.
    pub fn equalize(&mut self) {
        if let Node::Internal {
            ratio, left, right, ..
        } = self
        {
            *ratio = 0.5;
            left.equalize();
            right.equalize();
        }
    }

    /// Resize the split affecting a window in the given direction.
    /// Returns true if a resize was performed.
    pub fn resize(&mut self, window: XWindow, direction: Direction, delta: f32) -> bool {
        match self {
            Node::Leaf { .. } => false,
            Node::Internal {
                split,
                ratio,
                left,
                right,
            } => {
                // Check if this split is in the right orientation for the direction
                let dominated = match (split, direction) {
                    (SplitDirection::Vertical, Direction::Left | Direction::Right) => true,
                    (SplitDirection::Horizontal, Direction::Up | Direction::Down) => true,
                    _ => false,
                };

                if dominated {
                    // Check if the window is on the appropriate side
                    let in_left = left.contains(window);
                    let in_right = right.contains(window);

                    if in_left || in_right {
                        // Adjust ratio based on direction
                        let adjustment = match direction {
                            Direction::Right | Direction::Down => {
                                if in_left { delta } else { -delta }
                            }
                            Direction::Left | Direction::Up => {
                                if in_left { -delta } else { delta }
                            }
                        };

                        *ratio = (*ratio + adjustment).clamp(0.1, 0.9);
                        return true;
                    }
                }

                // Recurse into children
                if left.contains(window) {
                    left.resize(window, direction, delta)
                } else if right.contains(window) {
                    right.resize(window, direction, delta)
                } else {
                    false
                }
            }
        }
    }

    /// Get the split ratio affecting a window in the given direction.
    /// Returns None if no such split exists.
    pub fn get_split_ratio(&self, window: XWindow, direction: Direction) -> Option<f32> {
        match self {
            Node::Leaf { .. } => None,
            Node::Internal {
                split,
                ratio,
                left,
                right,
            } => {
                // Check if this split is in the right orientation for the direction
                let dominated = match (split, direction) {
                    (SplitDirection::Vertical, Direction::Left | Direction::Right) => true,
                    (SplitDirection::Horizontal, Direction::Up | Direction::Down) => true,
                    _ => false,
                };

                if dominated && (left.contains(window) || right.contains(window)) {
                    return Some(*ratio);
                }

                // Recurse into children
                if left.contains(window) {
                    left.get_split_ratio(window, direction)
                } else if right.contains(window) {
                    right.get_split_ratio(window, direction)
                } else {
                    None
                }
            }
        }
    }

    /// Set the split ratio affecting a window in the given direction.
    /// Returns true if the ratio was set.
    pub fn set_split_ratio(&mut self, window: XWindow, direction: Direction, new_ratio: f32) -> bool {
        match self {
            Node::Leaf { .. } => false,
            Node::Internal {
                split,
                ratio,
                left,
                right,
            } => {
                // Check if this split is in the right orientation for the direction
                let dominated = match (split, direction) {
                    (SplitDirection::Vertical, Direction::Left | Direction::Right) => true,
                    (SplitDirection::Horizontal, Direction::Up | Direction::Down) => true,
                    _ => false,
                };

                if dominated && (left.contains(window) || right.contains(window)) {
                    *ratio = new_ratio.clamp(0.1, 0.9);
                    return true;
                }

                // Recurse into children
                if left.contains(window) {
                    left.set_split_ratio(window, direction, new_ratio)
                } else if right.contains(window) {
                    right.set_split_ratio(window, direction, new_ratio)
                } else {
                    false
                }
            }
        }
    }

    /// Swap two windows in the tree.
    pub fn swap(&mut self, a: XWindow, b: XWindow) -> bool {
        // Find and swap the windows
        let mut found_a = false;
        let mut found_b = false;

        self.swap_impl(a, b, &mut found_a, &mut found_b);
        found_a && found_b
    }

    fn swap_impl(&mut self, a: XWindow, b: XWindow, found_a: &mut bool, found_b: &mut bool) {
        match self {
            Node::Leaf { window: Some(w) } => {
                if *w == a {
                    *w = b;
                    *found_a = true;
                } else if *w == b {
                    *w = a;
                    *found_b = true;
                }
            }
            Node::Leaf { window: None } => {}
            Node::Internal { left, right, .. } => {
                left.swap_impl(a, b, found_a, found_b);
                right.swap_impl(a, b, found_a, found_b);
            }
        }
    }

    /// Find adjacent window in a direction, given geometries.
    /// If `preferred` is Some and is a valid candidate, it will be returned.
    /// This enables "window memory" - remembering which window was last focused in a direction.
    pub fn find_adjacent(
        geometries: &[(XWindow, Rect)],
        from: XWindow,
        direction: Direction,
        preferred: Option<XWindow>,
    ) -> Option<XWindow> {
        let from_rect = geometries.iter().find(|(w, _)| *w == from)?.1;

        // Find the center point of the source window
        let from_cx = from_rect.x as i32 + from_rect.width as i32 / 2;
        let from_cy = from_rect.y as i32 + from_rect.height as i32 / 2;

        let candidates: Vec<_> = geometries
            .iter()
            .filter(|(w, rect)| {
                if *w == from {
                    return false;
                }

                let cx = rect.x as i32 + rect.width as i32 / 2;
                let cy = rect.y as i32 + rect.height as i32 / 2;

                match direction {
                    Direction::Left => cx < from_cx,
                    Direction::Right => cx > from_cx,
                    Direction::Up => cy < from_cy,
                    Direction::Down => cy > from_cy,
                }
            })
            .collect();

        // If preferred window is a valid candidate, use it (window memory)
        if let Some(pref) = preferred {
            if candidates.iter().any(|(w, _)| *w == pref) {
                return Some(pref);
            }
        }

        // Find the closest window in the direction
        // Prioritize alignment perpendicular to movement, then distance in movement direction
        candidates
            .into_iter()
            .min_by_key(|(_, rect)| {
                let cx = rect.x as i32 + rect.width as i32 / 2;
                let cy = rect.y as i32 + rect.height as i32 / 2;
                let dx = (cx - from_cx).abs();
                let dy = (cy - from_cy).abs();

                match direction {
                    // For left/right: prioritize same row (small dy), then closest x
                    Direction::Left | Direction::Right => dy * 100 + dx,
                    // For up/down: prioritize same column (small dx), then closest y
                    Direction::Up | Direction::Down => dx * 100 + dy,
                }
            })
            .map(|(w, _)| *w)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub fn new(x: i16, y: i16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn split(&self, direction: SplitDirection, ratio: f32) -> (Rect, Rect) {
        match direction {
            SplitDirection::Horizontal => {
                let split_y = (self.height as f32 * ratio) as u16;
                (
                    Rect::new(self.x, self.y, self.width, split_y),
                    Rect::new(
                        self.x,
                        self.y + split_y as i16,
                        self.width,
                        self.height.saturating_sub(split_y),
                    ),
                )
            }
            SplitDirection::Vertical => {
                let split_x = (self.width as f32 * ratio) as u16;
                (
                    Rect::new(self.x, self.y, split_x, self.height),
                    Rect::new(
                        self.x + split_x as i16,
                        self.y,
                        self.width.saturating_sub(split_x),
                        self.height,
                    ),
                )
            }
        }
    }
}
