#!/usr/bin/env python3
"""Open or reuse a develop-to-main PR without modifying either branch."""
import json
import os
import subprocess


def gh(*arguments):
    return subprocess.check_output(["gh", *arguments], text=True).strip()


def prepare(repository):
    pulls = json.loads(gh("pr", "list", "--repo", repository, "--base", "main",
                          "--head", "develop", "--state", "open", "--json", "url"))
    if pulls:
        return "Promotion PR: " + pulls[0]["url"]
    comparison = json.loads(gh("api", f"repos/{repository}/compare/main...develop"))
    if comparison["ahead_by"] == 0:
        return "No changes to promote from develop to main."
    url = gh("pr", "create", "--repo", repository, "--base", "main", "--head", "develop",
             "--title", "chore(release): promote develop to main for the next release",
             "--body", "Promote develop to main in preparation for the next release.\n\n"
             "Required checks and review must pass before merging. Use a merge commit "
             "to preserve branch ancestry; do not squash or rebase.\n\n"
             "This PR does not bump the version. Prepare Release creates a separate "
             "version and changelog PR after promotion.")
    return "Promotion PR: " + url


if __name__ == "__main__":
    result = prepare(os.environ["GITHUB_REPOSITORY"])
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as summary:
        summary.write(result + "\n")
