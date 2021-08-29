//! Intrusive [order statistic][1] [red-black][2] [tree][3]
//!
//! [1]: https://en.wikipedia.org/wiki/Order_statistic_tree
//! [2]: https://en.wikipedia.org/wiki/Red%E2%80%93black_tree
//! [3]: https://en.wikipedia.org/wiki/Binary_search_tree
//!
//! # Panic Safety
//!
//! **The mutation methods are not panic safe.**
//! If any of the mutation methods panic, the tree structure might get
//! corrupted, which will cause an undefined behavior in subsequent operations.
#![allow(unsafe_op_in_unsafe_fn)] // *terrified bookhorse noise*
use core::{
    cmp::Ordering,
    marker::PhantomPinned,
    mem::{replace, swap},
    ptr::NonNull,
};
use guard::guard;

#[cfg(not(debug_assertions))]
use core::hint::unreachable_unchecked;
#[cfg(debug_assertions)]
#[track_caller]
fn unreachable_unchecked() -> ! {
    unreachable!();
}

/// A node.
///
/// The operation functions mutate linked nodes behind a raw pointer. This is
/// obviously `!Unpin`.
pub struct Node<Element, Summary> {
    children: [Option<NonNull<Self>>; 2],
    parent: Option<NonNull<Self>>,
    color: Color,
    pub summary: Summary,
    pub element: Element,
    _pin: PhantomPinned,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Color {
    Black,
    Red,
}

type IsRightChild = bool;

pub trait Callback<Element, Summary> {
    /// Create a zero element of `Summary`.
    fn zero_summary(&mut self) -> Summary;
    /// Create a `Summary` from the given `Element`.
    fn element_to_summary(&mut self, element: &Element) -> Summary;
    /// Add `rhs` to `lhs` in place.
    fn add_assign_summary(&mut self, lhs: &mut Summary, rhs: &Summary);
    /// Subtract `rhs` from `lhs` in place.
    fn sub_assign_summary(&mut self, lhs: &mut Summary, rhs: &Summary);
    /// Determine the ordering between a new `Element` and an existing `Element`.
    /// If the result is `Equal`, it's assumed to be `Greater`, i.e., when
    /// duplicate elements are inserted, they are sorted in the insertion order.
    fn cmp_element(&mut self, new_element: &Element, existing_element: &Element) -> Ordering;
}

#[derive(Debug, Copy, Clone)]
pub struct DefaultCallback;

impl<Element: Ord> Callback<Element, ()> for DefaultCallback {
    #[inline]
    fn zero_summary(&mut self) {}
    #[inline]
    fn element_to_summary(&mut self, _element: &Element) {}
    #[inline]
    fn add_assign_summary(&mut self, _lhs: &mut (), _rhs: &()) {}
    #[inline]
    fn sub_assign_summary(&mut self, _lhs: &mut (), _rhs: &()) {}
    #[inline]
    fn cmp_element(&mut self, e1: &Element, e2: &Element) -> Ordering {
        e1.cmp(e2)
    }
}

impl<Element, Summary> Node<Element, Summary> {
    pub const fn new(element: Element, summary: Summary) -> Self {
        Self {
            children: [None, None],
            parent: None,
            color: Color::Black,
            summary,
            element,
            _pin: PhantomPinned,
        }
    }
}

impl<Element, Summary: Clone> Node<Element, Summary> {
    /// Insert `new_node` to a tree.
    ///
    /// If there's a node whose key is equal to the one given, the new node will
    /// be inserted after that node.
    ///
    /// It's up to the caller to initialize the new node's [`Node::summary`]
    /// with a value consistent with [`Callback::element_to_summary`]. Note that
    /// **a node that was just removed from a tree contains a summary value that
    /// is incorrect for re-insertion!**
    ///
    /// # Safety
    ///
    ///  - The tree and all included nodes must be (still) valid.
    ///  - `new_node` must not be already included in the tree. (If it's already
    ///    part of another tree, that tree will be corrupted.)
    ///  - All existing nodes in the tree and `new_node` are considered to be
    ///    mutably borrowed throughout the duration of the function call.
    ///
    pub unsafe fn insert(
        mut callback: impl Callback<Element, Summary>,
        tree: &mut Option<NonNull<Self>>,
        mut new_node: NonNull<Self>,
    ) {
        new_node.as_mut().color = Color::Red;
        new_node.as_mut().children = [None, None];

        // Find the initial place for `new_node`
        let (mut parent, mut node_side): (NonNull<Self>, IsRightChild);
        let mut grandparent_parent_side: Option<(NonNull<Self>, IsRightChild)> = None;
        if let Some(root) = *tree {
            parent = root;
            loop {
                // While we are on it, update the summaries of the potential
                // ancestor nodes
                debug_assert_ne!(parent, new_node);
                callback
                    .add_assign_summary(&mut parent.as_mut().summary, &new_node.as_ref().summary);

                // Find the place for the node
                // Warning: `cmp_element` has a more strict panic safety
                // requirement. The red-black tree invariants must be intact
                // when it's called.
                let side = callback
                    .cmp_element(&new_node.as_ref().element, &parent.as_ref().element)
                    != Ordering::Less;
                let child_cell = &mut parent.as_mut().children[side as usize];

                if let Some(child) = *child_cell {
                    // We need to go deeper
                    grandparent_parent_side = Some((parent, side));
                    parent = child;
                } else {
                    // Break the loop and into an uplifting musical number
                    *child_cell = Some(new_node);
                    new_node.as_mut().parent = Some(parent);
                    node_side = side;
                    break;
                }
            }
        } else {
            // `new_node` is the new root
            *tree = Some(new_node);
            new_node.as_mut().parent = None;
            return;
        }

        let mut node = new_node;
        loop {
            debug_assert_eq!(node.as_ref().color, Color::Red);
            debug_assert_eq!(parent.as_ref().children[node_side as usize], Some(node));
            debug_assert_eq!(node.as_ref().parent, Some(parent));

            // Color invariant fulfilled?
            if parent.as_ref().color == Color::Black {
                break;
            }

            // `parent` is red, so `node` cannot be red. What do we do now?
            if let Some((mut grandparent, parent_side)) = grandparent_parent_side {
                debug_assert_eq!(
                    grandparent.as_ref().children[parent_side as usize],
                    Some(parent)
                );
                debug_assert_eq!(parent.as_ref().parent, Some(grandparent));

                // Due to the color invariant, `grandparent` must be black.
                debug_assert_eq!(grandparent.as_ref().color, Color::Black);

                let uncle = grandparent.as_ref().children[(!parent_side) as usize];
                if let Some(mut uncle) = uncle.filter(|u| u.as_ref().color == Color::Red) {
                    // Both `parent` and `uncle` are red. Repaint them to black
                    // and `grandparent` to red. (This doesn't change
                    // `grandparent`'s subtree's black height.)
                    parent.as_mut().color = Color::Black;
                    uncle.as_mut().color = Color::Black;
                    grandparent.as_mut().color = Color::Red;

                    // `grandparent` might now violate the color invariant, so
                    // change `node` and iterate again
                    node = grandparent;
                    parent = if let Some(parent) = node.as_ref().parent {
                        node_side = parent.as_ref().children[1] == Some(node);
                        parent
                    } else {
                        break;
                    };
                    grandparent_parent_side = parent.as_ref().parent.map(|grandparent| {
                        (
                            grandparent,
                            grandparent.as_ref().children[1] == Some(parent),
                        )
                    });
                    continue;
                }

                // `parent` is red, but `uncle` is black. Check the sides of
                // `node` and `uncle`. If they are on the same side, we need to
                // fix this before proceeding to the next step.
                // (Note: A nil node is considered to be black.)
                if parent_side != node_side {
                    Self::rotate(&mut callback, parent, !node_side, tree);

                    // The rotation flips the relationship between `node` and
                    // `parent`.
                    swap(&mut parent, &mut node);
                    node_side = !node_side;
                    debug_assert_eq!(parent.as_ref().parent, Some(grandparent));
                    debug_assert_eq!(parent.as_ref().children[node_side as usize], Some(node));
                    debug_assert_eq!(node.as_ref().parent, Some(parent));
                }

                // Now `node` and `uncle` are on different sides.
                // Push `grandparent` to `uncle`'s position,
                // making `parent` (red) the parent of `node` (red) and
                // `grandparent` (black).
                Self::rotate(&mut callback, grandparent, !node_side, tree);

                // The paths through `node` now has one less black nodes,
                // violating the invariant. Repaint `parent` to black and
                // `grandparent` to red.
                parent.as_mut().color = Color::Black;
                grandparent.as_mut().color = Color::Red;
            } else {
                // grandparent == None, parent == root
                // Switch `parent`'s color, increasing the black height by one and
                // restoring the color invariant
                parent.as_mut().color = Color::Black;
            }

            break;
        }
    }

    /// Remove `node` from a tree.
    ///
    /// # Safety
    ///
    ///  - The tree and all included nodes must be (still) valid.
    ///  - `node` must be already included in the tree.
    ///  - All existing nodes in the tree and `new_node` are considered to be
    ///    mutably borrowed throughout the duration of the function call.
    ///
    pub unsafe fn remove(
        mut callback: impl Callback<Element, Summary>,
        tree: &mut Option<NonNull<Self>>,
        mut node: NonNull<Self>,
    ) {
        // Exclude `node.element` from the summaries.
        {
            let local_summary = callback.element_to_summary(&node.as_ref().element);
            let mut node = Some(node);
            while let Some(mut n) = node {
                callback.sub_assign_summary(&mut n.as_mut().summary, &local_summary);
                node = n.as_ref().parent;
            }
        }

        match (node.as_ref().parent, node.as_ref().children) {
            (None, [None, None]) => {
                // `node` is the only remaining node
                debug_assert_eq!(*tree, Some(node));
                *tree = None;
                return;
            }
            (parent, [Some(child), Some(_)]) => {
                // Swap `node` and `child`'s maximum element
                // (`node`'s in-order predecessor). This temporarily breaks
                // ordering.
                let mut pred = child;
                let mut pred_parent = node;
                let mut pred_side: IsRightChild = false;

                while let Some(next_pred) = pred.as_ref().children[1] {
                    pred_parent = pred;
                    pred = next_pred;
                    pred_side = true;
                }

                // `pred` will be replaced with `node` with zero summary. Deduct
                // `element_to_summary(pred)` from all nodes between `node` and
                // `pred` (exclusive).
                //
                // Don't do this when the tree is in an inconsistent state.
                // `callback`'s methods may panic!
                {
                    let mut mid = pred;
                    let local_summary = callback.element_to_summary(&pred.as_ref().element);
                    while let Some(mut mid_parent) = mid.as_ref().parent {
                        if mid_parent == node {
                            break;
                        }
                        callback
                            .sub_assign_summary(&mut mid_parent.as_mut().summary, &local_summary);
                        mid = mid_parent;
                    }
                }

                debug_assert_ne!(node, pred);
                swap(&mut node.as_mut().color, &mut pred.as_mut().color);
                swap(&mut node.as_mut().summary, &mut pred.as_mut().summary);
                if pred_parent != node {
                    debug_assert_eq!(pred_side, true);
                    swap(&mut node.as_mut().children, &mut pred.as_mut().children);
                    swap(&mut node.as_mut().parent, &mut pred.as_mut().parent);
                    pred_parent.as_mut().children[1] = Some(node);
                    if let Some(mut child) = pred.as_ref().children[0] {
                        child.as_mut().parent = Some(pred);
                    }
                } else {
                    debug_assert_eq!(pred_side, false);
                    debug_assert_eq!(pred, child);
                    swap(
                        &mut node.as_mut().children[1],
                        &mut pred.as_mut().children[1],
                    );
                    node.as_mut().children[0] = pred.as_ref().children[0];
                    pred.as_mut().children[0] = Some(node);
                    pred.as_mut().parent = node.as_ref().parent;
                    node.as_mut().parent = Some(pred);
                }
                if let Some(mut child) = pred.as_ref().children[1] {
                    child.as_mut().parent = Some(pred);
                }
                if let Some(mut child) = node.as_ref().children[0] {
                    child.as_mut().parent = Some(node);
                }
                let pred_position = if let Some(mut parent) = parent {
                    let node_side = parent.as_ref().children[1] == Some(node);
                    &mut parent.as_mut().children[node_side as usize]
                } else {
                    &mut *tree
                };
                debug_assert_eq!(*pred_position, Some(node));
                *pred_position = Some(pred);
            }
            (_, [_, None]) => {}
            (_, [None, Some(_)]) => {
                // Swap the children (the ordering doesn't matter because
                // `node` will cease to exist)
                node.as_mut().children.swap(0, 1);
            }
        }

        debug_assert_eq!(node.as_ref().children[1], None);

        // The slot that stores `node`
        // Warning: This mutably borrows `node.parent`!
        let (node_position, mut node_side) = if let Some(mut parent) = node.as_ref().parent {
            let node_side = parent.as_ref().children[1] == Some(node);
            (&mut parent.as_mut().children[node_side as usize], node_side)
        } else {
            (&mut *tree, false)
        };
        debug_assert_eq!(*node_position, Some(node));

        match (node.as_ref().color, node.as_ref().children[0]) {
            (Color::Red, child) => {
                // If `child` is non-nil, it must be black as per the color
                // invariant. However, having a black child at this position
                // would violate the black height invariant. Therefore,
                // `child` is nil, and `node` can be simply removed.
                debug_assert_eq!(child, None);

                *node_position = None;

                return;
            }
            (Color::Black, Some(mut child)) => {
                // `child` must be red because of the black height invariant.
                // Move `child` to `node`'s position and repaint it black.
                debug_assert_eq!(child.as_ref().color, Color::Red);
                child.as_mut().parent = node.as_ref().parent;
                child.as_mut().color = Color::Black;

                *node_position = Some(child);

                return;
            }
            (Color::Black, None) => {
                // We need to work hard to preserve the black height invariant
                // in this case...
            }
        }

        // Remove `node` anyway. This will decrement its ancestors' black
        // height, violating the black height invariant. We will restore
        // this invariant by traveling up the tree.
        *node_position = None;

        // If `parent` is `None`, the black height change propagated up to
        // the root
        guard!(let Some(mut parent) = node.as_ref().parent else { return; });

        loop {
            //       parent
            //        /   \
            //       /     \
            //     node  sibling
            //            /   \
            //           /     \
            //  close_nephew distant_nephew
            //
            // `node`'s sibling must exist because of the black height invariant
            let mut sibling = parent.as_ref().children[(!node_side) as usize].unwrap();
            let close_nephew = sibling.as_ref().children[node_side as usize];
            let distant_nephew = sibling.as_ref().children[(!node_side) as usize];

            match (
                parent.as_ref().color,
                close_nephew.map(|n| (n, n.as_ref().color)),
                sibling.as_ref().color,
                distant_nephew.map(|n| (n, n.as_ref().color)),
            ) {
                // Due to the black height invariant, there must be at least
                // one black in `sibling`'s side's path
                (_, None, Color::Red, _) | (_, _, Color::Red, None) => unreachable_unchecked(),
                // Due to the color invariant
                (_, Some((_, Color::Red)), Color::Red, _)
                | (_, _, Color::Red, Some((_, Color::Red))) => unreachable_unchecked(),
                (Color::Red, _, Color::Red, _) => unreachable_unchecked(),
                // Valid cases
                (Color::Black, Some((_, Color::Black)), Color::Red, Some((_, Color::Black))) => {
                    // Move `parent` into `node`'s position. `parent` adopts
                    // `close_nephew`.
                    Self::rotate(&mut callback, parent, node_side, tree);

                    // Repaint `parent` and `sibling` (now grandparent) to red
                    // and black
                    parent.as_mut().color = Color::Red;
                    sibling.as_mut().color = Color::Black;

                    // `close_nephew` (now sibling) is black, so if we
                    // iterate again, we will fall through to the below cases
                }
                (
                    Color::Black,
                    None | Some((_, Color::Black)),
                    Color::Black,
                    None | Some((_, Color::Black)),
                ) => {
                    // Repaint `sibling` to red. This rectifies the black height
                    // difference between `node` and `sibling`. However,
                    // `parent` still has one less black height than the rest
                    // of the tree.
                    sibling.as_mut().color = Color::Red;

                    // However, `parent` still has one less black height than
                    // the rest of the tree.
                    node = parent;

                    parent = if let Some(parent) = node.as_ref().parent {
                        node_side = parent.as_ref().children[1] == Some(node);
                        debug_assert_eq!(parent.as_ref().children[node_side as usize], Some(node));
                        parent
                    } else {
                        // The black height change propagated up to the root
                        break;
                    };
                }
                (
                    Color::Red,
                    None | Some((_, Color::Black)),
                    Color::Black,
                    None | Some((_, Color::Black)),
                ) => {
                    // Repaint `parent` and `sibling` to black and red. This
                    // restores `node`'s black height and keeps `sibling`'s
                    // intact.
                    parent.as_mut().color = Color::Black;
                    sibling.as_mut().color = Color::Red;
                    break;
                }
                (_, _, Color::Black, Some((mut distant_nephew, Color::Red))) => {
                    // Move `sibling` to `parent`'s position. `parent` adopts
                    // `close_nephew`.
                    Self::rotate(&mut callback, parent, node_side, tree);

                    // Swap `sibling` and `parent`'s colors
                    swap(&mut sibling.as_mut().color, &mut parent.as_mut().color);

                    distant_nephew.as_mut().color = Color::Black;
                    break;
                }
                (_, Some((mut close_nephew, Color::Red)), Color::Black, _) => {
                    // Move `close_newphew` to `sibling`'s position.
                    Self::rotate(&mut callback, sibling, !node_side, tree);

                    sibling.as_mut().color = Color::Red;
                    close_nephew.as_mut().color = Color::Black;

                    // `sibling` (now distant nephew) is red, so if we iterate
                    // again, we will take the above case
                }
            } // match
        } // loop
    }

    /// Rotate a node. `dir` specifies `node`'s position after rotation.
    unsafe fn rotate(
        callback: &mut impl Callback<Element, Summary>,
        mut node: NonNull<Self>,
        dir: IsRightChild,
        tree: &mut Option<NonNull<Self>>,
    ) {
        let idir = (!dir) as usize;
        let dir = dir as usize;

        //          node            new_root
        //          /  \            /  \
        //         /    \          /    \
        //  new_root    y   ==>   x     node
        //    /  \                      /  \
        //   x  mid                    mid  y

        let mut new_root = node.as_ref().children[idir].expect("post-rotation root does not exist");
        let mid = new_root.as_mut().children[dir];
        node.as_mut().children[idir] = mid;
        new_root.as_mut().children[dir] = Some(node);

        new_root.as_mut().parent = node.as_ref().parent;
        node.as_mut().parent = Some(new_root);
        if let Some(mut mid) = mid {
            mid.as_mut().parent = Some(node);
        }

        // Update the subtree's parent's child pointer.
        let child_cell = if let Some(mut parent) = new_root.as_ref().parent {
            let children = &mut parent.as_mut().children;
            if children[0] == Some(node) {
                &mut children[0]
            } else {
                &mut children[1]
            }
        } else {
            tree
        };
        debug_assert_eq!(*child_cell, Some(node));
        *child_cell = Some(new_root);

        // Update the summaries. Do this after transforming the tree structure
        // for panic safety.
        // (new_root.summary, node.summary) =
        //    (node.summary, node.summary - new_root.summary + mid.summary)
        let mut node_sumamry_new = node.as_ref().summary.clone();
        callback.sub_assign_summary(&mut node_sumamry_new, &new_root.as_ref().summary);
        if let Some(mid) = mid {
            callback.add_assign_summary(&mut node_sumamry_new, &mid.as_ref().summary);
        }
        new_root.as_mut().summary = replace(&mut node.as_mut().summary, node_sumamry_new);
    }

    /// Find the minimum (leftmost) node in the subtree rooted by `this`.
    ///
    /// # Safety
    ///
    ///  - The tree and all included nodes must be (still) valid.
    ///  - All traversed nodes are considered to be borrowed throughout the
    ///    duration of the function call.
    ///
    #[inline]
    pub unsafe fn min(mut this: NonNull<Self>) -> NonNull<Self> {
        loop {
            if let Some(child) = this.as_ref().children[0] {
                this = child;
            } else {
                return this;
            }
        }
    }

    /// Find the maximum (rightmost) node in the subtree rooted by `this`.
    ///
    /// # Safety
    ///
    ///  - The tree and all included nodes must be (still) valid.
    ///  - All traversed nodes are considered to be borrowed throughout the
    ///    duration of the function call.
    ///
    #[inline]
    pub unsafe fn max(mut this: NonNull<Self>) -> NonNull<Self> {
        loop {
            if let Some(child) = this.as_ref().children[1] {
                this = child;
            } else {
                return this;
            }
        }
    }

    /// Find the in-order predecessor of `this`.
    ///
    /// # Safety
    ///
    ///  - The tree and all included nodes must be (still) valid.
    ///  - All traversed nodes are considered to be borrowed throughout the
    ///    duration of the function call.
    ///
    #[inline]
    pub unsafe fn predecessor(this: NonNull<Self>) -> Option<NonNull<Self>> {
        let mut node = this.as_ref();

        if let Some(child) = node.children[0] {
            return Some(Self::max(child));
        }

        loop {
            if let Some(parent) = &node.parent {
                let parent = parent.as_ref();
                if parent.children[1] == Some(NonNull::from(node)) {
                    return Some(parent.into());
                } else {
                    node = parent;
                }
            } else {
                // There's none
                return None;
            }
        }
    }

    /// Find the in-order successor of `this`.
    ///
    /// # Safety
    ///
    ///  - The tree and all included nodes must be (still) valid.
    ///  - All traversed nodes are considered to be borrowed throughout the
    ///    duration of the function call.
    ///
    #[inline]
    pub unsafe fn successor(this: NonNull<Self>) -> Option<NonNull<Self>> {
        let mut node = this.as_ref();

        if let Some(child) = node.children[1] {
            return Some(Self::min(child));
        }

        loop {
            if let Some(parent) = &node.parent {
                let parent = parent.as_ref();
                if parent.children[0] == Some(NonNull::from(node)) {
                    return Some(parent.into());
                } else {
                    node = parent;
                }
            } else {
                // There's none
                return None;
            }
        }
    }

    /// Find the minimum element that is not less than a key. The comparison
    /// between a given key and elements is defined by the provided closure.
    ///
    /// The comparison must be consistent with [`Callback::cmp_element`]. (I.e.,
    /// the returned value must change from `Greater` to `Equal` to `Less` as
    /// the key increases.)
    ///
    /// # Safety
    ///
    ///  - The tree and all included nodes must be (still) valid.
    ///  - All existing nodes in the tree and `new_node` are considered to be
    ///    borrowed throughout the duration of the function call.
    ///
    pub unsafe fn lower_bound(
        tree: &Option<NonNull<Self>>,
        mut cmp: impl FnMut(&Element) -> Ordering,
    ) -> Option<NonNull<Self>> {
        let mut node = if let Some(node) = tree {
            node.as_ref()
        } else {
            return None;
        };
        loop {
            match cmp(&node.element) {
                Ordering::Less | Ordering::Equal => {
                    if let Some(child) = &node.children[0] {
                        node = child.as_ref();
                    } else {
                        return Some(node.into());
                    }
                }
                Ordering::Greater => {
                    if let Some(child) = &node.children[1] {
                        node = child.as_ref();
                    } else {
                        // Find the in-order successor
                        return Self::successor(node.into());
                    }
                }
            }
        }
    }

    /// Return the summary of nodes that precedes the specified node `node`.
    /// If `node` is `None`, the summary of all nodes in the tree will be
    /// returned.
    ///
    /// # Safety
    ///
    ///  - The tree and all included nodes must be (still) valid.
    ///  - `
    ///  - All existing nodes in the tree and `new_node` are considered to be
    ///    borrowed throughout the duration of the function call.
    ///
    pub unsafe fn prefix_sum(
        mut callback: impl Callback<Element, Summary>,
        tree: &Option<NonNull<Self>>,
        node: Option<NonNull<Self>>,
    ) -> Summary
    where
        Summary: Clone,
    {
        let mut node = if let Some(node) = node {
            node.as_ref()
        } else if let Some(root) = *tree {
            // There's none; return the total sum
            return root.as_ref().summary.clone();
        } else {
            // There's none because the tree is empty
            return callback.zero_summary();
        };

        // Calculate the rank of the node
        let mut summary = if let Some(child) = node.children[0] {
            child.as_ref().summary.clone()
        } else {
            callback.zero_summary()
        };
        loop {
            if let Some(parent) = &node.parent {
                let parent = parent.as_ref();
                if parent.children[1] == Some(NonNull::from(node)) {
                    if let Some(child) = parent.children[0] {
                        callback.add_assign_summary(&mut summary, &child.as_ref().summary);
                    }

                    let local_summary = callback.element_to_summary(&parent.element);
                    callback.add_assign_summary(&mut summary, &local_summary);
                }
                node = parent;
            } else {
                break;
            }
        }
        summary
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use quickcheck_macros::quickcheck;
    use std::{
        cell::UnsafeCell,
        collections::{BTreeSet, HashMap},
        prelude::v1::*,
    };

    impl<Element: std::fmt::Debug, Summary: Clone + std::fmt::Debug + PartialEq>
        Node<Element, Summary>
    {
        unsafe fn iter(p_node: Option<NonNull<Self>>) -> impl Iterator<Item = NonNull<Self>> {
            let mut cursor = p_node.map(|pn| Self::min(pn));
            std::iter::from_fn(move || {
                cursor.map(|current| {
                    cursor = Node::successor(current);
                    current
                })
            })
        }

        unsafe fn iter_rev(p_node: Option<NonNull<Self>>) -> impl Iterator<Item = NonNull<Self>> {
            let mut cursor = p_node.map(|pn| Self::max(pn));
            std::iter::from_fn(move || {
                cursor.map(|current| {
                    cursor = Node::predecessor(current);
                    current
                })
            })
        }

        pub(crate) unsafe fn dump(
            p_node: Option<NonNull<Self>>,
            level: usize,
            out: &mut impl std::fmt::Write,
        ) -> std::fmt::Result {
            for _ in 0..level {
                write!(out, "  ")?;
            }
            if let Some(p_node) = p_node {
                let node = p_node.as_ref();

                writeln!(
                    out,
                    "{:?} {:?} {:?} {:?}",
                    p_node, node.element, node.summary, node.color
                )?;
                for &child in node.children.iter() {
                    Self::dump(child, level + 1, out)?;
                }
            } else {
                writeln!(out, "nil")?;
            }
            Ok(())
        }

        pub(crate) unsafe fn validate(
            callback: &mut impl Callback<Element, Summary>,
            tree: &Option<NonNull<Self>>,
        ) {
            let mut black_path = Vec::new();
            let mut visited_path = Vec::new();
            let mut found_black_path: Option<Vec<_>> = None;
            if let Some(node) = *tree {
                Self::validate_node(
                    callback,
                    node,
                    &mut black_path,
                    &mut |complete_black_path| {
                        if let Some(found_black_path) = &found_black_path {
                            assert_eq!(
                                complete_black_path.len(),
                                found_black_path.len(),
                                "black path length mismatch. an example:\n - {:?}\n - {:?}",
                                complete_black_path,
                                found_black_path
                            );
                        } else {
                            found_black_path = Some(complete_black_path.to_owned());
                        }
                    },
                    &mut visited_path,
                );
            }

            // Check duplicates
            let mut counts = HashMap::new();
            for p_node in Self::iter(*tree) {
                *counts.entry(p_node).or_insert(0usize) += 1;
            }

            for (&ptr, &count) in counts.iter() {
                assert_eq!(
                    count, 1,
                    "node {:?} appear in the tree for {} times",
                    ptr, count
                );
            }
        }

        unsafe fn validate_node(
            callback: &mut impl Callback<Element, Summary>,
            p_node: NonNull<Self>,
            black_path: &mut Vec<NonNull<Self>>,
            report_path: &mut impl FnMut(&[NonNull<Self>]),
            visited_path: &mut Vec<NonNull<Self>>,
        ) {
            let node = p_node.as_ref();

            if let Some(p_parent) = node.parent {
                let parent = p_parent.as_ref();
                assert!(
                    parent.children.contains(&Some(p_node)),
                    "parent.children {:?} does not contain {:?}",
                    parent.children,
                    p_node,
                );

                if parent.color == Color::Red {
                    assert_eq!(
                        node.color,
                        Color::Black,
                        "a red node must not have a red child",
                    );
                }
            }

            // The tree must not be circular
            assert!(
                !visited_path.contains(&p_node),
                "tree is circular: {:?}",
                visited_path
            );

            if node.color == Color::Black {
                black_path.push(p_node);
            }
            visited_path.push(p_node);

            let mut summary = callback.element_to_summary(&node.element);

            for &child in node.children.iter() {
                if let Some(child) = child {
                    Self::validate_node(callback, child, black_path, report_path, visited_path);
                    callback.add_assign_summary(&mut summary, &child.as_ref().summary);
                } else {
                    report_path(&black_path);
                }
            }

            if node.color == Color::Black {
                black_path.pop().unwrap();
            }
            visited_path.pop().unwrap();

            assert!(
                !(summary != node.summary),
                "summary mismatch at {:?}. expected: {:?}, got: {:?}",
                p_node,
                summary,
                node.summary,
            );
        }
    }

    #[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy)]
    struct TestElement(u8, usize);

    type TestSummary = u64;

    struct TestCallback;

    impl Callback<TestElement, u64> for TestCallback {
        fn zero_summary(&mut self) -> TestSummary {
            0
        }
        fn element_to_summary(&mut self, element: &TestElement) -> TestSummary {
            element.0 as _
        }

        fn add_assign_summary(&mut self, lhs: &mut TestSummary, rhs: &TestSummary) {
            *lhs += *rhs;
        }

        fn sub_assign_summary(&mut self, lhs: &mut TestSummary, rhs: &TestSummary) {
            *lhs -= *rhs;
        }

        fn cmp_element(&mut self, e1: &TestElement, e2: &TestElement) -> Ordering {
            // `.1` is determined by an insertion order, so nodes should be
            // implicitly ordered by `.1`.
            e1.0.cmp(&e2.0)
        }
    }

    #[quickcheck]
    fn qc_btree(cmds: Vec<u8>) {
        let mut cmds = cmds.into_iter();
        let mut nodes = Vec::<Box<UnsafeCell<Node<_, _>>>>::new();
        let mut inserted_node_is = Vec::<usize>::new();
        let mut expected = BTreeSet::new();
        let mut keys_worth_checking = BTreeSet::new();
        let mut tree = None;

        log::info!("Command: {:?}", cmds);

        (|| -> Option<()> {
            while let Some(cmd) = cmds.next() {
                match cmd % 2 {
                    0 if !inserted_node_is.is_empty() => {
                        let node_i =
                            inserted_node_is.swap_remove(cmd as usize % inserted_node_is.len());
                        let node = &nodes[node_i];
                        let element = unsafe { (*node.get()).element };
                        let node_ptr = NonNull::new(node.get()).unwrap();
                        log::debug!("Remove {:?}", element);

                        unsafe { Node::remove(TestCallback, &mut tree, node_ptr) };

                        expected.remove(&element);
                        // TODO: remove `nodes[node_i]`
                    }
                    _ => {
                        let element = TestElement(cmds.next()?, nodes.len());
                        let summary = TestCallback.element_to_summary(&element);

                        let node = Box::new(UnsafeCell::new(Node::new(element, summary)));
                        let node_ptr = NonNull::new(node.get()).unwrap();
                        inserted_node_is.push(nodes.len());
                        nodes.push(node);
                        log::debug!("Insert {:?} as {:?}", element, node_ptr);

                        unsafe { Node::insert(TestCallback, &mut tree, node_ptr) };

                        expected.insert(element);
                        keys_worth_checking.insert(element.0);
                    }
                }

                {
                    let mut st = String::new();
                    unsafe { Node::dump(tree, 1, &mut st).unwrap() };
                    log::trace!("Tree = \n{}", st);
                }

                // Validate the tree after each command
                unsafe { Node::validate(&mut TestCallback, &tree) };

                // Check the sequence represented
                let expected: Vec<_> = expected.iter().cloned().collect();

                let tree_elements: Vec<_> = unsafe {
                    Node::iter(tree)
                        .map(|p_node| p_node.as_ref().element)
                        .collect()
                };
                assert_eq!(tree_elements, expected);

                let mut tree_elements: Vec<_> = unsafe {
                    Node::iter_rev(tree)
                        .map(|p_node| p_node.as_ref().element)
                        .collect()
                };
                tree_elements.reverse();
                assert_eq!(tree_elements, expected);

                // Check the prefix sums
                for &key in keys_worth_checking.iter() {
                    let lower_bound = unsafe { Node::lower_bound(&tree, |e| key.cmp(&e.0)) };
                    let got_prefix_sum: TestSummary =
                        unsafe { Node::prefix_sum(TestCallback, &tree, lower_bound) };
                    let expected_prefix_sum: TestSummary = expected
                        .iter()
                        .filter(|element| element.0 < key)
                        .map(|element| element.0 as TestSummary)
                        .sum();
                    assert_eq!(
                        got_prefix_sum, expected_prefix_sum,
                        "prefix_sum({}) = {} (expected = {})",
                        key, got_prefix_sum, expected_prefix_sum
                    );
                }
            }

            Some(())
        })();
    }

    // TODO: test panic safety
}
