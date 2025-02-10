/*
The idea behind is to have high efficient access to the graph thanks to the Hashmap.
The HashMap has a key role also because it provides info about the state.
The state follow the following rule:
(0) - an edge that is still to be resolved.
(1) - an edge you CAN contact
(2) - an edge you cannot contact


state will be accessed likely only by the edges, while nodeIndex is for the access to the graph (that is done automatically);
hence, state checks will still be performed by the edges!
*/

use petgraph::graph::{NodeIndex};
use std::collections::HashMap;
use petgraph::stable_graph::StableUnGraph;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::NodeType;
use wg_2024::packet::NodeType::Drone;
use crate::DEBUG_MODE;

type State = u8;

#[derive(Debug, Clone)]
pub struct Node {
    id: NodeId,
    node_type: Option<NodeType>,
    forward_count: u32,
    dropped_count: u32,
}

impl Node {
    fn reliability(&self) -> f64 {
        self.forward_count as f64 / (self.forward_count + self.dropped_count) as f64
    }
}

#[derive(Debug)]
pub struct Network {
    pub graph: StableUnGraph<Node, ()>,
    node_map: HashMap<NodeId, (State, NodeIndex)>,
}

impl Network {
    pub fn new() -> Self {
        Self {
            graph: StableUnGraph::default(),
            node_map: HashMap::new(),
        }
    }

    /// Adds the nodes and updates existent ones.
    pub fn add_route(&mut self, src: NodeId, nodes: Vec<(NodeId, NodeType)>) {
        let mut prev_index = None;

        for (id, node_type) in nodes {
            let node_index = match self.node_map.get(&id) {
                Some(&(_, existing_index)) => {
                    // If node already present, I return just the index.
                    self.graph[existing_index].node_type = Some(node_type);
                    existing_index
                },
                None => {
                    // New node, we add it.
                    let node = Node {
                        id,
                        node_type: Some(node_type),
                        forward_count: 1,
                        dropped_count: 0,
                    };
                    let index = self.graph.add_node(node);
                    let state = match node_type{
                        Drone => {2u8}
                        _ => {
                            if id == src {
                                2u8
                            } else {
                                0u8
                            }
                        }
                    };
                    self.node_map.insert(id, (state, index));
                    index
                }
            };

            // Connect adjacent nodes.
            if let Some(prev) = prev_index {
                if !self.graph.contains_edge(prev, node_index) {
                    self.graph.add_edge(prev, node_index, ());
                }
                if !self.graph.contains_edge(node_index, prev) {
                    self.graph.add_edge(node_index, prev, ());
                }
            }

            prev_index = Some(node_index);
        }
    }
    
    pub fn get_indexes_from_vec(&self, ids: Vec<NodeId>) -> Option<Vec<NodeIndex>> {
        ids.iter()
            .map(|id| self.node_map.get(id).map(|&(_, idx)| idx))
            .collect()
    }

    // Function to calculate the path's weight.
    pub fn calculate_path_weight (&self, path: Vec<NodeIndex>) -> f64 {
         path.iter()
        .map(|&node_index| self.graph[node_index].reliability())
        .product()
    }


    /// Find the best path to dst from start
    pub fn best_path(&self, start: &NodeId, end: &NodeId) -> Option<(Vec<NodeId>, f64)> {
        let start_index = self.node_map.get(start)?.1;
        let end_index = self.node_map.get(end)?.1;
        
        
        let mut stack = vec![(start_index, vec![start_index])];
        let mut max_weight = f64::MIN;
        let mut best_path_indexes: Option<Vec<NodeIndex>> = None;
        let mut best_path_length = usize::MAX;
        
        
        while let Some((current, path)) = stack.pop() {
            if current == end_index {
                let weight = self.calculate_path_weight(path.clone());
                if weight > max_weight || (weight == max_weight && path.len() < best_path_length) {
                    max_weight = weight;
                    best_path_length = path.len();
                    best_path_indexes = Some(path.clone());
                }
            } else {
                for neighbor in self.graph.neighbors(current) {
                    // Check if this neighbor is already in the current path
                    if !path.contains(&neighbor) {
                        let mut new_path = path.clone();
                        new_path.push(neighbor);
                        stack.push((neighbor, new_path));
                    }
                }
            }
        }


        // Convert the best path from NodeIndex to NodeId, if it exists.
        best_path_indexes.map(|indexes| (indexes.into_iter().map(|idx| self.graph[idx].id).collect(), max_weight))
    }


    /// Adds a forward count to all nodes in the route.
    pub fn positive_feedback(&mut self, route: Vec<NodeId>) {
        for node_id in route {
            if let Some(&(_, index)) = self.node_map.get(&node_id) {
                if self.graph[index].node_type == Some(Drone) {
                    self.graph[index].forward_count += 1;
                }
            }
        }
    }

    /// Increase dropped count of the indicated node.
    pub fn negative_feedback(&mut self, dst: NodeId) {
        if let Some(&(_, index)) = self.node_map.get(&dst) {
            if self.graph[index].node_type == Some(Drone) {
                self.graph[index].dropped_count += 1;
            }
        }
        self.check_for_100();
    }
    pub fn update_state(&mut self, dst: NodeId, new_state: u8) {
        if let Some((state,_ )) = self.node_map.get_mut(&dst) {
            *state = new_state;
        }
    }
    pub fn remove_node(&mut self, src: NodeId){
        if let Some((_, ind)) = self.node_map.remove(&src){
            for neigh in self.graph.clone().neighbors(ind){
                self.remove_faulty_connection(src, self.graph[neigh].id);
            }

            self.graph.remove_node(ind);
        }
    }

    pub fn remove_faulty_connection(&mut self, start: NodeId, dst: NodeId) {
        if let (Some(&(_, index_a)), Some(&(_, index_b))) =
            (self.node_map.get(&start), self.node_map.get(&dst))
        {
            if let Some(edge) = self.graph.find_edge(index_a, index_b) {
                self.graph.remove_edge(edge);
            }
        }
    }

    pub fn get_srh(&self, start: &NodeId, end: &NodeId) -> Option<SourceRoutingHeader> {
        if let Some((vec, _)) = self.best_path(start, end){
            Some(SourceRoutingHeader {
                hop_index: 1,
                hops: vec
            })
        }else {
            None
        }
    }

    pub fn get_state(&self, dst: &NodeId) -> Option<State>{
       self.node_map.get(dst).map(|&s| s.0)
    }
    
    pub fn get_unresolved(&self) -> Vec<NodeId> {
        self.node_map
            .iter()
            .filter_map(|(id, (state, _))| {
                if *state == 0 {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect()
    }
    
    pub fn get_optimal_dest (&mut self, start: &NodeId, v: &Vec<NodeId>) -> Option<NodeId> {
        let mut out: Option<NodeId> = None;
        let mut weight = f64::MAX;
        for i in v {
            if let Some((r, r_weight)) = self.best_path(start, i){
                if weight > r_weight && !r.is_empty() {
                    out = Some(r[r.len() - 1]);
                    weight = r_weight;
                }
            }
        }

        if DEBUG_MODE{
            println!("{:?} is your best destination for this", out);
        }
        out
    }
    
    pub fn has_all_routes(&self, source_id: NodeId) -> bool {
        if self.node_map.is_empty(){
            return false;
        }

        for (id,_) in self.node_map.iter() {
            match self.best_path(&source_id, id) {
                Some((_, _)) => {},
                None => return false
            }
        }

        true
    }

    pub fn add_destination_without_path(&mut self, dst_id: NodeId) {
        match self.node_map.get(&dst_id) {
            Some(&(_state, _existing_index)) => {
                // If node already present, I don't need to do anything.
            },
            None => {
                // New node, we add it.
                let node = Node {
                    id: dst_id,
                    node_type: None,
                    forward_count: 1,
                    dropped_count: 0,
                };
                let index = self.graph.add_node(node);
                // The state is unknown.
                self.node_map.insert(dst_id, (0u8, index));
            }
        }
    }
    ///double check
    pub fn check_for_100(&mut self) {
        let mut to_remove = Vec::new();
        for (id, (_, index)) in &self.node_map {
            let node = &self.graph[*index];
            if node.reliability() < 0.02 {
                to_remove.push(*id);
            }
            // Should be 0.01, but it's probably good to keep it slightly higher.
        }

        for id in to_remove {
            self.remove_node(id);
        }
    }
}


