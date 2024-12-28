use std::collections::HashMap;
use wg_2024::network::{NodeId, SourceRoutingHeader};

#[derive(Clone)]
pub struct Route {
    path: Vec<NodeId>,
}
pub struct RouteList {
    routes: Vec<Route>,
}

impl Route {
    pub fn new(path: Vec<NodeId>) -> Route {
        Route { path }
    }
    pub fn get_cost(&self) -> usize {
        self.path.len()
    }
    pub fn to_source_routing_header(&self) -> SourceRoutingHeader {
        SourceRoutingHeader {
            hop_index: 1,
            hops: self.path.clone(),
        }
    }
    pub fn contains_node(&self, node_id: &NodeId) -> bool {
        self.path.contains(node_id)
    }
}

impl RouteList {
    pub fn new() -> RouteList {
        RouteList { routes: Vec::new() }
    }
    pub fn add_route(&mut self, route: Route) {
        self.routes.push(route);
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

    pub fn get_fastest_route(&self) -> Option<Route> {
        let mut res = None;
        for (route) in self.routes.iter() {
            if res.is_none() {
                res = Some(route.clone());
            } else if route.get_cost() < res.as_ref()?.get_cost() {
                res = Some(route.clone());
            }
        }
        res // Will result as None if there are no more routes cut of errors.
    }
}
