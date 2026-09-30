//! The compositor on the native target: change rows and their timing, late
//! joiners and frame-parallel workers, the bypass, the geometry, motion,
//! fonts and images.

mod common;

mod bypass;
mod fonts;
mod geometry;
mod images;
mod joiner;
mod motion;
mod parallel;
mod rows;
mod timing;
