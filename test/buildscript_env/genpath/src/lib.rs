// The include! is the point: it resolves the env var at compile time, so a
// stale path fails the build rather than producing a wrong answer.
include!(env!("GENERATED_ANSWER_PATH"));

pub fn plain() -> &'static str {
    env!("PLAIN_VALUE")
}
