# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](http://keepachangelog.com/en/1.0.0/)
and this project adheres to [Semantic Versioning](http://semver.org/spec/v2.0.0.html).

## [Unreleased]
## [0.0.2] - 2021-09-10

- Added
	- `interlock::hl::slice::*RbTree{SliceRef, Vec}IntervalRwLock` - while the existing `*RbTreeSliceIntervalRwLock` encompasses them, its use of projection causes [inconveniences][1] with the current non-Chalk trait solver.
	- `interlock::hl::slice::SliceIntervalRwLock::{, async_, try_}{read, write}_boxed`
	- `interlock::hl::slice::SliceIntervalRwLock::update_container`

## 0.0.1 - 2021-09-09

[Unreleased]: https://github.com/yvt/interlock-rs/compare/0.0.2...HEAD
[0.0.2]: https://github.com/yvt/interlock-rs/compare/0.0.1...0.0.2
[1]: https://github.com/rust-lang/rust/issues/85849
