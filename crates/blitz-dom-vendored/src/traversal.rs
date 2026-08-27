use std::cmp::Ordering;

use style::{dom::TNode as _, values::specified::box_::DisplayInside};

use crate::{BaseDocument, Node};

macro_rules! iter_children {
    ($node_expr:expr, $cb:expr) => {{
        let node = &mut $node_expr;
        let children = core::mem::take(&mut node.children);
        for child_id in children.iter().copied() {
            $cb(child_id)
        }
        $node_expr.children = children;
    }};
}
pub(crate) use iter_children;

macro_rules! iter_children_and_pseudos {
    ($node_expr:expr, $cb:expr) => {{
        // Load node
        let node = &mut $node_expr;

        // Copy before, after, and take children
        let before = node.before;
        let after = node.after;
        let children = core::mem::take(&mut node.children);

        if let Some(before) = before {
            $cb(before)
        }
        for child_id in children.iter().copied() {
            $cb(child_id)
        }
        if let Some(after) = after {
            $cb(after)
        }

        // Reload node and put children back
        $node_expr.children = children;
    }};
}
pub(crate) use iter_children_and_pseudos;

#[derive(Clone)]
/// An pre-order tree traverser for a [BaseDocument](crate::document::BaseDocument).
pub struct TreeTraverser<'a> {
    doc: &'a BaseDocument,
    stack: Vec<usize>,
}

impl<'a> TreeTraverser<'a> {
    /// Creates a new tree traverser for the given document which starts at the root node.
    pub fn new(doc: &'a BaseDocument) -> Self {
        Self::new_with_root(doc, 0)
    }

    /// Creates a new tree traverser for the given document which starts at the specified node.
    pub fn new_with_root(doc: &'a BaseDocument, root: usize) -> Self {
        let mut stack = Vec::with_capacity(32);
        stack.push(root);
        TreeTraverser { doc, stack }
    }
}
impl Iterator for TreeTraverser<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.stack.pop()?;
        let node = self.doc.get_node(id)?;
        self.stack.extend(node.children.iter().rev());
        Some(id)
    }
}

#[derive(Clone)]
/// An ancestor traverser for a [BaseDocument](crate::document::BaseDocument).
pub struct AncestorTraverser<'a> {
    doc: &'a BaseDocument,
    current: usize,
}
impl<'a> AncestorTraverser<'a> {
    /// Creates a new ancestor traverser for the given document and node ID.
    pub fn new(doc: &'a BaseDocument, node_id: usize) -> Self {
        AncestorTraverser {
            doc,
            current: node_id,
        }
    }
}
impl Iterator for AncestorTraverser<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        let current_node = self.doc.get_node(self.current)?;
        self.current = current_node.parent?;
        Some(self.current)
    }
}

impl Node {
    #[allow(dead_code)]
    pub(crate) fn should_traverse_layout_children(&mut self) -> bool {
        let prefer_layout_children = match self.display_constructed_as.inside() {
            DisplayInside::None => return false,
            DisplayInside::Contents => false,
            DisplayInside::Flow | DisplayInside::FlowRoot | DisplayInside::TableCell => {
                // Prefer layout children for "block" but not "inline" contexts
                self.element_data()
                    .is_none_or(|el| el.inline_layout_data.is_none())
            }
            DisplayInside::Flex | DisplayInside::Grid => true,
            DisplayInside::Table => false,
            DisplayInside::TableRowGroup => false,
            DisplayInside::TableColumn => false,
            DisplayInside::TableColumnGroup => false,
            DisplayInside::TableHeaderGroup => false,
            DisplayInside::TableFooterGroup => false,
            DisplayInside::TableRow => false,
        };
        let has_layout_children = self.layout_children.get_mut().is_some();
        prefer_layout_children & has_layout_children
    }
}

impl BaseDocument {
    /// Collect the nodes into a chain by traversing upwards
    pub fn node_chain(&self, node_id: usize) -> Vec<usize> {
        let mut chain = Vec::with_capacity(16);
        chain.push(node_id);
        chain.extend(
            AncestorTraverser::new(self, node_id).filter(|id| self.nodes[*id].is_element()),
        );
        chain
    }

    pub fn visit<F>(&self, mut visit: F)
    where
        F: FnMut(usize, &Node),
    {
        TreeTraverser::new(self).for_each(|node_id| visit(node_id, &self.nodes[node_id]));
    }

    /// If the node is non-anonymous then returns the node's id
    /// Else find's the first non-anonymous ancester of the node
    pub fn non_anon_ancestor_if_anon(&self, mut node_id: usize) -> usize {
        loop {
            let node = &self.nodes[node_id];

            if !node.is_anonymous() {
                return node.id;
            }

            let Some(parent_id) = node.layout_parent.get() else {
                // Shouldn't be reachable unless invalid node_id is passed
                // as root node is always non-anonymous
                panic!("Node does not exist or does not have a non-anonymous parent");
            };

            node_id = parent_id;
        }
    }

    pub fn iter_children_mut(
        &mut self,
        node_id: usize,
        mut cb: impl FnMut(usize, &mut BaseDocument),
    ) {
        let children = std::mem::take(&mut self.nodes[node_id].children);
        for child_id in children.iter().cloned() {
            cb(child_id, self);
        }
        self.nodes[node_id].children = children;
    }

    pub fn iter_subtree_mut(
        &mut self,
        node_id: usize,
        mut cb: impl FnMut(usize, &mut BaseDocument),
    ) {
        cb(node_id, self);
        iter_subtree_mut_inner(self, node_id, &mut cb);
        fn iter_subtree_mut_inner(
            doc: &mut BaseDocument,
            node_id: usize,
            cb: &mut impl FnMut(usize, &mut BaseDocument),
        ) {
            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().cloned() {
                cb(child_id, doc);
                iter_subtree_mut_inner(doc, child_id, cb);
            }
            doc.nodes[node_id].children = children;
        }
    }

    pub fn iter_children_and_pseudos_mut(
        &mut self,
        node_id: usize,
        mut cb: impl FnMut(usize, &mut BaseDocument),
    ) {
        let before = self.nodes[node_id].before.take();
        if let Some(before_node_id) = before {
            cb(before_node_id, self)
        }
        self.nodes[node_id].before = before;

        self.iter_children_mut(node_id, &mut cb);

        let after = self.nodes[node_id].after.take();
        if let Some(after_node_id) = after {
            cb(after_node_id, self)
        }
        self.nodes[node_id].after = after;
    }

    pub fn next_node(&self, start: &Node, mut filter: impl FnMut(&Node) -> bool) -> Option<usize> {
        let start_id = start.id;
        let mut node = start;
        let mut look_in_children = true;
        loop {
            // Next is first child
            let next = if look_in_children && !node.children.is_empty() {
                let node_id = node.children[0];
                &self.nodes[node_id]
            }
            // Next is next sibling or parent
            else if let Some(parent) = node.parent_node() {
                let self_idx = parent
                    .children
                    .iter()
                    .position(|id| *id == node.id)
                    .unwrap();
                // Next is next sibling
                if let Some(sibling_id) = parent.children.get(self_idx + 1) {
                    look_in_children = true;
                    &self.nodes[*sibling_id]
                }
                // Next is parent
                else {
                    look_in_children = false;
                    node = parent;
                    continue;
                }
            }
            // Continue search from the root
            else {
                look_in_children = true;
                self.root_node()
            };

            if filter(next) {
                return Some(next.id);
            } else if next.id == start_id {
                return None;
            }

            node = next;
        }
    }

    pub fn node_layout_ancestors(&self, node_id: usize) -> Vec<usize> {
        let mut ancestors = Vec::with_capacity(12);
        let mut maybe_id = Some(node_id);
        while let Some(id) = maybe_id {
            ancestors.push(id);
            maybe_id = self.nodes[id].layout_parent.get();
        }
        ancestors.reverse();
        ancestors
    }

    pub fn maybe_node_layout_ancestors(&self, node_id: Option<usize>) -> Vec<usize> {
        node_id
            .map(|id| self.node_layout_ancestors(id))
            .unwrap_or_default()
    }

    /// Compare the document order of two nodes.
    /// Returns Ordering::Less if node_a comes before node_b in document order.
    /// Returns Ordering::Greater if node_a comes after node_b.
    /// Returns Ordering::Equal if they are the same node.
    pub fn compare_document_order(&self, node_a: usize, node_b: usize) -> Ordering {
        if node_a == node_b {
            return Ordering::Equal;
        }

        // Build ancestor chains from root to node (inclusive)
        let chain_a = self.ancestor_chain_from_root(node_a);
        let chain_b = self.ancestor_chain_from_root(node_b);

        // Find where the chains diverge
        let mut common_depth = 0;
        for (a, b) in chain_a.iter().zip(chain_b.iter()) {
            if a != b {
                break;
            }
            common_depth += 1;
        }

        // If one is an ancestor of the other
        if common_depth == chain_a.len() {
            return Ordering::Less; // node_a is ancestor of node_b
        }
        if common_depth == chain_b.len() {
            return Ordering::Greater; // node_b is ancestor of node_a
        }

        // IGNITION PATCH: the chains only share a root when both nodes are
        // still attached to the document. A node that has been removed by a
        // re-render (`remove_node` leaves it parentless) has a chain that
        // starts at itself, so `common_depth` is 0 and the upstream
        // `chain_a[common_depth - 1]` underflows. Two nodes with no common
        // ancestor have no document order; report `Equal` instead of panicking.
        if common_depth == 0 {
            return Ordering::Equal;
        }

        // Compare position among siblings at the divergence point
        let divergent_a = chain_a[common_depth];
        let divergent_b = chain_b[common_depth];
        let parent_id = chain_a[common_depth - 1];
        let parent = &self.nodes[parent_id];

        for &child_id in &parent.children {
            if child_id == divergent_a {
                return Ordering::Less;
            }
            if child_id == divergent_b {
                return Ordering::Greater;
            }
        }

        // Should not reach here if tree is well-formed
        Ordering::Equal
    }

    /// IGNITION PATCH: whether `node_id` exists and its parent chain reaches
    /// the root node (node 0). A node removed from the tree keeps its slab
    /// slot until it is dropped, so a stale id can still resolve to a node.
    pub fn is_attached_to_root(&self, node_id: usize) -> bool {
        let mut current = Some(node_id);
        while let Some(id) = current {
            let Some(node) = self.nodes.get(id) else {
                return false;
            };
            if id == 0 {
                return true;
            }
            current = node.parent;
        }
        false
    }

    /// Build ancestor chain from root to node (inclusive), ordered [root, ..., node].
    fn ancestor_chain_from_root(&self, node_id: usize) -> Vec<usize> {
        let mut ancestors = Vec::with_capacity(16);
        let mut current = Some(node_id);
        while let Some(id) = current {
            ancestors.push(id);
            current = self.nodes[id].parent;
        }
        ancestors.reverse();
        ancestors
    }

    /// Collect all inline root nodes between start_node and end_node in document order.
    /// Both start and end are assumed to be inline roots.
    /// Returns the nodes in document order (from first to last).
    pub fn collect_inline_roots_in_range(&self, start_node: usize, end_node: usize) -> Vec<usize> {
        // Resolve nodes: for anonymous blocks, get (parent_id, Some(anon_id)); for regular, (node_id, None)
        let (start_anchor, start_anon) = self.resolve_for_traversal(start_node);
        let (end_anchor, end_anon) = self.resolve_for_traversal(end_node);

        // IGNITION PATCH: a selection endpoint whose node was removed from
        // the document (still allocated, but with no path to the root) has
        // no range to collect. Bail out rather than walk a detached subtree.
        if !self.is_attached_to_root(start_anchor) || !self.is_attached_to_root(end_anchor) {
            return Vec::new();
        }

        // If both are anonymous blocks with the same parent, just collect from layout_children
        if start_anon.is_some() && end_anon.is_some() && start_anchor == end_anchor {
            return self.collect_anonymous_siblings(start_anchor, start_node, end_node);
        }

        // Determine first/last based on document order (using anchors for comparison)
        let (first_anchor, first_anon, last_anchor, last_anon) = match self
            .compare_document_order(start_anchor, end_anchor)
        {
            Ordering::Less | Ordering::Equal => (start_anchor, start_anon, end_anchor, end_anon),
            Ordering::Greater => (end_anchor, end_anon, start_anchor, start_anon),
        };

        let mut result = Vec::new();
        let mut found_first = false;

        // Traverse tree in document order
        for node_id in TreeTraverser::new(self) {
            if !found_first {
                if node_id == first_anchor {
                    found_first = true;
                    if let Some(anon_id) = first_anon {
                        // First is anonymous: collect from this parent starting at anon_id
                        // Stop at last_anchor if different parent, or last_anon if same parent
                        let stop_at = if first_anchor == last_anchor {
                            // Same parent: stop at last_anon
                            last_anon
                        } else {
                            // Different parents: stop at last_anchor (which is a child of first_anchor)
                            Some(last_anchor)
                        };
                        self.collect_layout_children_inline_roots(
                            node_id,
                            Some(anon_id),
                            stop_at,
                            &mut result,
                        );
                        // If we collected up to last, we're done
                        if result.last() == Some(&last_anchor)
                            || last_anon.is_some_and(|la| result.last() == Some(&la))
                        {
                            break;
                        }
                        continue;
                    }
                }
            }

            if found_first {
                if node_id == last_anchor {
                    if let Some(anon_id) = last_anon {
                        // Last is anonymous: collect up to anon_id (exclusive), then include anon_id
                        self.collect_layout_children_inline_roots(
                            node_id,
                            None,
                            Some(anon_id),
                            &mut result,
                        );
                        // Include the last_anon itself (until is exclusive, so we add it here)
                        if !result.contains(&anon_id) {
                            result.push(anon_id);
                        }
                    } else {
                        // Last is regular: include it if it's an inline root and not already collected
                        let node = &self.nodes[node_id];
                        if node.flags.is_inline_root() && !result.contains(&node_id) {
                            result.push(node_id);
                        }
                    }
                    break;
                }

                let node = &self.nodes[node_id];
                if node.flags.is_inline_root() && !result.contains(&node_id) {
                    result.push(node_id);
                } else {
                    // For non-inline-root nodes, collect any inline roots from their layout_children
                    // This handles intermediate block containers with anonymous block children
                    self.collect_layout_children_inline_roots(
                        node_id,
                        None,
                        Some(last_anchor),
                        &mut result,
                    );
                }
            }
        }

        result
    }

    /// Resolve a node for traversal purposes.
    /// For anonymous blocks: returns (parent_id, Some(node_id))
    /// For regular nodes: returns (node_id, None)
    fn resolve_for_traversal(&self, node_id: usize) -> (usize, Option<usize>) {
        let node = &self.nodes[node_id];
        if node.is_anonymous() {
            (node.parent.unwrap_or(node_id), Some(node_id))
        } else {
            (node_id, None)
        }
    }

    /// Collect anonymous block siblings between start and end (inclusive)
    /// Also recursively collects inline roots from any block children in between
    fn collect_anonymous_siblings(&self, parent_id: usize, start: usize, end: usize) -> Vec<usize> {
        let parent = &self.nodes[parent_id];
        let layout_children = parent.layout_children.borrow();
        let Some(children) = layout_children.as_ref() else {
            return Vec::new();
        };

        let start_idx = children.iter().position(|&id| id == start);
        let end_idx = children.iter().position(|&id| id == end);

        let (first_idx, last_idx) = match (start_idx, end_idx) {
            (Some(s), Some(e)) if s <= e => (s, e),
            (Some(s), Some(e)) => (e, s),
            _ => return Vec::new(),
        };

        let mut result = Vec::new();
        for &child_id in &children[first_idx..=last_idx] {
            let child = &self.nodes[child_id];
            if child.flags.is_inline_root() {
                result.push(child_id);
            } else {
                // For non-inline-root children (block containers), collect all their inline roots
                self.collect_all_inline_roots_in_subtree(child_id, &mut result);
            }
        }
        result
    }

    /// Recursively collect all inline roots from a node's layout_children subtree
    fn collect_all_inline_roots_in_subtree(&self, node_id: usize, result: &mut Vec<usize>) {
        let node = &self.nodes[node_id];
        let layout_children = node.layout_children.borrow();
        let Some(children) = layout_children.as_ref() else {
            return;
        };

        for &child_id in children.iter() {
            let child = &self.nodes[child_id];
            if child.flags.is_inline_root() {
                result.push(child_id);
            } else {
                // Recurse into block children
                self.collect_all_inline_roots_in_subtree(child_id, result);
            }
        }
    }

    /// Collect inline roots from a parent's layout_children.
    /// - `from`: If Some, start collecting from this node; if None, start from beginning
    /// - `until`: If Some, stop when we reach this node OR a node that contains it; if None, collect to end
    fn collect_layout_children_inline_roots(
        &self,
        parent_id: usize,
        from: Option<usize>,
        until: Option<usize>,
        result: &mut Vec<usize>,
    ) {
        let parent = &self.nodes[parent_id];
        let layout_children = parent.layout_children.borrow();
        let Some(children) = layout_children.as_ref() else {
            return;
        };

        let mut collecting = from.is_none(); // Start immediately if no 'from' specified
        for &child_id in children.iter() {
            if from == Some(child_id) {
                collecting = true;
            }
            if collecting {
                // Stop without adding if this child contains the 'until' node (it will be processed later)
                if let Some(until_id) = until {
                    if self.is_ancestor_of(child_id, until_id) {
                        break;
                    }
                }
                // Stop before processing if this child IS the 'until' node
                if until == Some(child_id) {
                    break;
                }
                let child = &self.nodes[child_id];
                if child.flags.is_inline_root() {
                    result.push(child_id);
                } else {
                    // For non-inline-root children (block containers), recursively collect their inline roots
                    self.collect_all_inline_roots_in_subtree(child_id, result);
                }
            }
        }
    }

    /// Check if `ancestor_id` is an ancestor of `descendant_id`
    fn is_ancestor_of(&self, ancestor_id: usize, descendant_id: usize) -> bool {
        let mut current = descendant_id;
        while let Some(parent) = self.nodes[current].parent {
            if parent == ancestor_id {
                return true;
            }
            current = parent;
        }
        false
    }
}

// IGNITION PATCH: regression tests for the detached-node crash.
#[cfg(test)]
mod detached_node_tests {
    use crate::{BaseDocument, DocumentConfig};
    use markup5ever::{QualName, local_name, ns};
    use std::cmp::Ordering;

    fn p() -> QualName {
        QualName::new(None, ns!(html), local_name!("p"))
    }

    /// Root with two paragraphs, each holding one text node. Returns
    /// (p1, text1, p2, text2).
    fn two_paragraphs(doc: &mut BaseDocument) -> (usize, usize, usize, usize) {
        let root = doc.root_node().id;
        let mut m = doc.mutate();
        let p1 = m.create_element(p(), vec![]);
        let t1 = m.create_text_node("one");
        let p2 = m.create_element(p(), vec![]);
        let t2 = m.create_text_node("two");
        m.append_children(p1, &[t1]);
        m.append_children(p2, &[t2]);
        m.append_children(root, &[p1, p2]);
        (p1, t1, p2, t2)
    }

    #[test]
    fn compare_document_order_tolerates_a_detached_node() {
        let mut doc = BaseDocument::new(DocumentConfig::default());
        let (p1, t1, _p2, t2) = two_paragraphs(&mut doc);
        assert_eq!(doc.compare_document_order(t1, t2), Ordering::Less);
        assert_eq!(doc.compare_document_order(t2, t1), Ordering::Greater);

        // Detach the first paragraph without dropping it, the way a
        // Dioxus re-render does between two pointer-move events.
        doc.mutate().remove_node(p1);
        assert!(!doc.is_attached_to_root(t1));
        assert!(doc.is_attached_to_root(t2));

        // Upstream indexed `chain[common_depth - 1]` with common_depth == 0.
        assert_eq!(doc.compare_document_order(t1, t2), Ordering::Equal);
        assert_eq!(doc.compare_document_order(t2, t1), Ordering::Equal);
    }

    #[test]
    fn collect_inline_roots_in_range_is_empty_for_a_detached_anchor() {
        let mut doc = BaseDocument::new(DocumentConfig::default());
        let (p1, t1, _p2, t2) = two_paragraphs(&mut doc);
        doc.mutate().remove_node(p1);
        assert!(doc.collect_inline_roots_in_range(t1, t2).is_empty());
        assert!(doc.collect_inline_roots_in_range(t2, t1).is_empty());
    }

    #[test]
    fn removing_a_selection_endpoint_clears_the_selection() {
        let mut doc = BaseDocument::new(DocumentConfig::default());
        let (p1, t1, _p2, t2) = two_paragraphs(&mut doc);
        doc.set_text_selection(t1, 0, t2, 1);
        assert!(doc.has_text_selection());

        doc.mutate().remove_node(p1);
        assert!(!doc.has_text_selection());
        assert!(doc.get_text_selection_ranges().is_empty());
        assert_eq!(doc.mousedown_node_id, None);
    }

    #[test]
    fn dropping_a_selection_endpoint_clears_the_selection() {
        let mut doc = BaseDocument::new(DocumentConfig::default());
        let (_p1, t1, p2, t2) = two_paragraphs(&mut doc);
        doc.set_text_selection(t1, 0, t2, 1);
        doc.set_mousedown_node_id(Some(t2));
        doc.mutate().remove_and_drop_node(p2);
        assert!(!doc.has_text_selection());
        assert_eq!(doc.mousedown_node_id, None);
    }
}
