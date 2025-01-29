use std::cell::RefCell;
use std::cmp::Ordering;
// use std::fmt;
use std::fmt::{Debug, Display};
use std::sync::Arc;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::NodeType;

#[derive(Clone, Debug)]
pub struct Node {
    id: NodeId,
    node_type: NodeType,
    arrived_packets: u64,
    dropped_packets: u64,
    // Every node keeps track of arrived and dropped messages.
    // This count doesn't consider errors different from the drop.
}
#[derive(Clone, Debug)]
pub struct Nodes {
    nodes: Vec<Arc<RefCell<Node>>>,
}
#[derive(Clone, Debug)]
pub struct Route {
    path: Vec<Arc<RefCell<Node>>>,
}
#[derive(Clone, Debug)]
pub struct RouteList {
    routes: Vec<Route>,
}

impl Node {
    pub fn new(id: NodeId, node_type: NodeType) -> Node {
        Node{id, arrived_packets: 1, dropped_packets: 0, node_type}
    }
    fn get_reliability(&self) -> f64 {
        (self.arrived_packets as f64)/(self.arrived_packets as f64 + self.dropped_packets as f64)
    }
    fn positive_feed(&mut self) {
        self.arrived_packets += 1;
    }
    fn negative_feed(&mut self) {
        self.dropped_packets += 1;
    }
    fn is_drone(&self) -> bool {
        match self.node_type {
            NodeType::Drone => true,
            _ => false,
        }
    }
}

impl Nodes {
    pub fn new() -> Nodes {
        Nodes{nodes: Vec::new()}
    }

    // I apply positive feed to all nodes in the received route.
    // I exclude nodes that are NOT drones, since they can't drop the packets.
    pub fn positive_feed(&mut self, route: Vec<NodeId>) {
        for i in route {
            for j in self.nodes.iter_mut() {
                if i == j.borrow().id && j.borrow().is_drone() {
                    j.borrow_mut().positive_feed();
                }
            }
        }
    }
    // I apply negative feed to the node that dropped the packet.
    // I exclude nodes that are NOT drones, since they can't drop the packets.
    pub fn negative_feed(&mut self, node: NodeId) {
        for i in self.nodes.iter_mut() {
            if node == i.borrow().id && i.borrow().is_drone() {
                i.borrow_mut().negative_feed();
            }
        }
    }

    pub fn remove_faulty_node(&mut self, faulty_node: NodeId) {
        self.nodes.retain(|node| node.borrow().id != faulty_node);
        // I only keep the others.
    }
}
/*
// !!Might still need to use this, since now there are Arcs and RefCells
impl Debug for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Route")
            .field("path", &self.get_path_debug())
            .finish()
    }
}*/

//i compare two routes based on their reliability
// so a > b means route a is more reliable than b
impl PartialEq<Self> for Route {
    fn eq(&self, other: &Self) -> bool {
        self.get_reliability() == other.get_reliability()
    }
}

impl PartialOrd for Route{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.get_reliability().partial_cmp(&other.get_reliability())
    }
}



impl Route {
    pub fn new(path: Vec<(NodeId, NodeType)>) -> Route {
        let path = path.iter().map(|x| {
            Arc::new(RefCell::new(Node::new(x.0, x.1)))
        }).collect();
        Route { path }
    }
    // !!Needed if we need the impl Debug
    /*pub fn get_path_debug(&self) -> String{
        let mut res = String::new();
        let _ = self.path.iter()
            .map(|x| {
                res.push_str(&x.borrow().id.to_string());
                res.push_str(" -> ");
            }).collect();
        res
    }*/
    fn get_reliability(&self) -> f64 {
        let mut reliability = 0.0;
        for node in self.path.iter() {
            reliability *= node.borrow().get_reliability();
        }
        reliability
        // By weighting the routes, we consider the drop rates.
    }
    pub fn to_source_routing_header(&self) -> SourceRoutingHeader {
        SourceRoutingHeader {
            hop_index: 1,
            hops: self.path
                .iter()
                .map(|(node)| node.borrow().id)
                .collect(),
        }
    }
    fn contains_node(&self, node_id: &NodeId) -> bool {
        self.path
            .iter()
            .position(|(node)| node.borrow().id == *node_id) != None
    }

    fn check_for_100_pdr(&self) -> Option<NodeId> {
        let mut res = None;
        for node in self.path.iter() {
            if node.borrow().arrived_packets == 1 && node.borrow().dropped_packets > 1000 {
                res = Some(node.borrow().id);
            }
        }
        res
    }

    fn is_equal_to_path (&self, nodes: Vec<NodeId>) -> bool {
        if self.path.len() != nodes.len() {
            return false;
        }

        for i in 0..nodes.len() {
            if let Some(a) = self.path.get(i) {
                if let Some(b) = nodes.get(i) {
                    if a.borrow().id != *b {
                        return false;
                    }
                } else {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }
}

impl RouteList {
    pub fn new() -> RouteList {
        RouteList { routes: Vec::new() }
    }
    pub fn add_route(&mut self, route: Route) {
        let vec = route.path.iter().map(|x| x.borrow().id).collect();
        if !self.contains_route(vec) {
            self.routes.push(route);
        }
    }

    pub fn remove_faulty_node(&mut self, node_id: NodeId) {
        self.routes = self
            .routes
            .clone()
            .into_iter()
            .filter(|x| !x.contains_node(&node_id))
            .collect();
        // If I made this correct, I only keep stuff that doesn't contain
        // the node that gives error.
    }

    pub fn get_fastest_route(&mut self) -> Option<Route> {
        /*
        todo!() fix

        */


        let mut res = None;
        let mut reliability: f64 = 0.0;
        let mut to_remove = Vec::new();
        for route in self.routes.iter() {

            if res.is_none() || route.get_reliability() > reliability{
                res = Some(route.clone());
                reliability = route.get_reliability();
            } else if route.get_reliability() < reliability {
                // Since this is called often, I put a check for nodes with PDR too high
                match route.check_for_100_pdr() {
                    None => {},
                    Some(delete_id) => {
                        to_remove.push(delete_id);
                    }
                }
            }
        }
        for e in to_remove.iter() {
            self.remove_faulty_node(*e);
        }
        res // Will result as None if there are no more routes cut of errors.
    }

    pub fn contains_route(&self, nodes: Vec<NodeId>) -> bool{
        for route in self.routes.iter() {
            if route.is_equal_to_path(nodes.clone()) {
                return true;
            }
        }

        false
    }
}
