use std::collections::HashMap;
use wg_2024::network::{NodeId, SourceRoutingHeader};

#[derive(Clone)]
pub struct Route {
    path: Vec<NodeId>,
}
pub struct RouteList {
    routes: HashMap<u8,Route>,
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
}

impl RouteList {
    pub fn new() -> RouteList {
        RouteList {
            routes: HashMap::new(),
        }
    }
    pub fn add_route(&mut self, route: Route) {
        let mut i = 0;
        loop {
            if self.routes.insert(i, route.clone()).is_some() {
                break;
            }else {
                i += 1;
            }
        }
    }

    pub fn remove_route(&mut self, route_id: u8) {
        self.routes.remove(&route_id);
    }

    pub fn get_fastest_route(&self) -> Option<Route> {
        let mut res = None;
        for (_, route) in self.routes.iter() {
            if res.is_none() {
                res = Some(route.clone());
            } else if (route.get_cost() < res.as_ref()?.get_cost()) {
                res = Some(route.clone());
            }
        }
        res
    }
}