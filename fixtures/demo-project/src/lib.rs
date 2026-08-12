/// Intentionally wrong — task A in the demo graph must fix this to `a + b`.
/// Leaving it broken makes the gate fail until the agent (or mock) repairs it.
pub fn add(a: i32, b: i32) -> i32 {
    a - b
}
