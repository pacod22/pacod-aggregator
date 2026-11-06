//! What pacod-aggregator's server and its scraper say to each other, and the rules both must
//! apply alike. Prices and properties are the catalogue's own types, so that what a page says
//! needs no translation on its way into the catalogue.

pub mod egress;
pub mod extraction;
pub mod hours;
pub mod jobs;
pub mod pages;
