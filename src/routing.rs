use std::fmt::{Display, Formatter};
use wg_2024::network::{NodeId, SourceRoutingHeader};

#[derive(Clone, Debug)]
pub struct Route {
    path: Vec<(NodeId, u64, u64)>,
    // Every node keeps track of arrived and dropped messages.
    // This count doesn't consider errors different from the drop.
}
#[derive(Clone, Debug)]
pub struct RouteList {
    routes: Vec<Route>,
}

impl Route {
    pub fn new(path: Vec<NodeId>) -> Route {
        let path = path
            .into_iter()
            .map(|x| (x, 1, 0))
            .collect::<Vec<(NodeId, u64, u64)>>();
        Route { path }
    }
    pub fn get_weight(&self) -> f64 {
        let mut weight = 0.0;
        for (_,x,y) in self.path.iter() {
            weight *= (*x as f64)/(*x as f64 + *y as f64);
        }
        weight
        // By weighting the routes, we should consider the drop rates
    }
    pub fn to_source_routing_header(&self) -> SourceRoutingHeader {
        SourceRoutingHeader {
            hop_index: 1,
            hops: self.path.iter().map(|(x,_,_)| *x).collect(),
        }
    }
    fn contains_node(&self, node_id: &NodeId) -> bool {
        self.path.iter().position(|(x,_,_)| x == node_id) != None
    }

    fn positive_feed(&mut self, node: NodeId) {
        self.path = self.path.iter().map(|(x,y,z)|
            if *x == node {
                (*x, *y+1, *z)
            } else {
                (*x, *y, *z)
            }
        ).collect();
    }
    fn negative_feed(&mut self, node: NodeId) {
        self.path = self.path.iter().map(|(x,y,z)|
            if *x == node {
                (*x, *y, *z+1)
            } else {
                (*x, *y, *z)
            }
        ).collect();
    }

    fn check_for_100_pdr(&self) -> Option<NodeId> {
        let mut res = None;
        for (x,y,z) in self.path.iter() {
            if *y == 1 && *z > 1000 {
                res = Some(*x);
            }
        }
        res
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

    pub fn get_fastest_route(&mut self) -> Option<Route> {
        let mut res = None;
        let mut weight: f64 = 0.0;
        let mut to_remove = Vec::new();
        for route in self.routes.iter() {

            if res.is_none() || route.get_weight() > weight{
                res = Some(route.clone());
                weight = route.get_weight();
            } else if route.get_weight() < weight {
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

    // I apply positive feed to all routes to this destination
    pub fn positive_feed(&mut self, route: Vec<NodeId>) {
        for i in route {
            for j in self.routes.iter_mut() {
                j.positive_feed(i);
            }
        }
    }
    // i apply negative feed to the node to all the routes with that node,
    pub fn negative_feed(&mut self, node: NodeId) {
        for i in self.routes.iter_mut() {
            if i.contains_node(&node) {
                i.negative_feed(node);
            }
        }
    }
}
