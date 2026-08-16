subinclude("//test/build_defs:e2e")

please_repo_e2e_test(
    name = "consumer_test",
    plz_command = "plz run //:hello && plz test //:hello_test",
    repo = "test_repo",
)
