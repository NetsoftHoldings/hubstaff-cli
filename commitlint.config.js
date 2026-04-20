export default {
    extends: [
        '@commitlint/config-conventional',
    ],
    rules: {
        "header-max-length": [2, "always", 100],
        "subject-case": [0],
        "type-enum": [
            2,
            "always",
            [
                "chore",
                "ci",
                "docs",
                "feat",
                "fix",
                "perf",
                "refactor",
                "revert",
                "style",
                "test",
                "build",
                "deps",
            ],
        ],
    },
}
