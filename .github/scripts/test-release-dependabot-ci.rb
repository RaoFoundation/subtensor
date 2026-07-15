#!/usr/bin/env ruby

require "minitest/autorun"
require_relative "release-dependabot-ci"

class FakeGitHub
  attr_reader :posts, :run_queries

  def initialize(heads: ["head"], reviews: approved_reviews, review_snapshots: nil,
                 runs: [[]], jobs: {}, permissions: {}, post_failures: [])
    @heads = heads.dup
    @last_head = @heads.last
    @review_snapshots = (review_snapshots || [reviews]).dup
    @last_reviews = @review_snapshots.last
    @runs = runs.dup
    @last_runs = @runs.last
    @jobs = jobs
    @permissions = { "alice" => "write", "bob" => "admin" }.merge(permissions)
    @post_failures = post_failures
    @posts = []
    @run_queries = 0
  end

  def get(path)
    case path
    when %r{/pulls/\d+$}
      head = @heads.length > 1 ? @heads.shift : @last_head
      { "head" => { "sha" => head }, "user" => { "login" => "dependabot[bot]" } }
    when %r{/collaborators/([^/]+)/permission$}
      { "permission" => @permissions.fetch(Regexp.last_match(1), "read") }
    else
      raise "unexpected GET #{path}"
    end
  end

  def pages(path)
    case path
    when %r{/reviews\?}
      reviews = @review_snapshots.length > 1 ? @review_snapshots.shift : @last_reviews
      [reviews]
    when %r{/actions/runs\?}
      @run_queries += 1
      runs = @runs.length > 1 ? @runs.shift : @last_runs
      [{ "workflow_runs" => runs }]
    when %r{/actions/runs/(\d+)/attempts/1/jobs\?}
      [{ "jobs" => @jobs.fetch(Regexp.last_match(1).to_i, []) }]
    else
      raise "unexpected paged GET #{path}"
    end
  end

  def post(path)
    run_id = path[%r{/runs/(\d+)/}, 1].to_i
    @posts << run_id
    raise "simulated POST failure" if @post_failures.include?(run_id)
  end

  def self.approved_reviews
    [
      review("alice", "APPROVED", "head", "2026-01-01T00:00:00Z"),
      review("bob", "APPROVED", "head", "2026-01-01T00:00:01Z")
    ]
  end

  def self.review(user, state, commit_id, submitted_at)
    {
      "user" => { "login" => user }, "state" => state,
      "commit_id" => commit_id, "submitted_at" => submitted_at
    }
  end

  def self.run(id, attempt: 1, status: "completed", conclusion: "failure",
               workflow_id: id, pr_number: 42)
    {
      "id" => id, "run_attempt" => attempt, "status" => status,
      "conclusion" => conclusion, "workflow_id" => workflow_id,
      "pull_requests" => [{ "number" => pr_number }],
      "created_at" => "2026-01-01T00:00:#{id % 60}.000Z"
    }
  end

  def self.gate_failure
    [{ "name" => DependabotCiRelease::GATE_JOB, "conclusion" => "failure" }]
  end

  private

  def approved_reviews
    self.class.approved_reviews
  end
end

class DependabotCiReleaseTest < Minitest::Test
  def controller(github, **options)
    DependabotCiRelease.new(
      github: github, repository: "RaoFoundation/subtensor", pr_number: 42,
      review_sha: "head", max_wait_attempts: 3, wait_seconds: 0,
      sleeper: ->(_seconds) {}, **options
    )
  end

  def test_releases_only_first_attempt_runs_whose_gate_failed
    runs = [
      FakeGitHub.run(101),
      FakeGitHub.run(102),
      FakeGitHub.run(103, attempt: 2)
    ]
    github = FakeGitHub.new(
      runs: [runs, runs],
      jobs: {
        101 => FakeGitHub.gate_failure,
        102 => [{ "name" => "cargo test", "conclusion" => "failure" }]
      }
    )

    controller(github).run

    assert_equal [101], github.posts
  end

  def test_aborts_when_head_changes_before_release
    github = FakeGitHub.new(
      heads: %w[head changed],
      runs: [[FakeGitHub.run(101)], [FakeGitHub.run(101)]],
      jobs: { 101 => FakeGitHub.gate_failure }
    )

    error = assert_raises(DependabotCiRelease::Error) { controller(github).run }

    assert_match(/refusing to release stale CI/, error.message)
    assert_empty github.posts
  end

  def test_waits_for_first_attempt_gate_runs_to_settle
    github = FakeGitHub.new(
      runs: [
        [FakeGitHub.run(101, status: "queued", conclusion: nil)],
        [FakeGitHub.run(101)],
        [FakeGitHub.run(101)]
      ],
      jobs: { 101 => FakeGitHub.gate_failure }
    )

    controller(github).run

    assert_equal 3, github.run_queries
    assert_equal [101], github.posts
  end

  def test_attempts_every_release_before_reporting_partial_failure
    github = FakeGitHub.new(
      runs: [
        [FakeGitHub.run(101), FakeGitHub.run(104)],
        [FakeGitHub.run(101), FakeGitHub.run(104)]
      ],
      jobs: { 101 => FakeGitHub.gate_failure, 104 => FakeGitHub.gate_failure },
      post_failures: [101]
    )

    error = assert_raises(DependabotCiRelease::Error) { controller(github).run }

    assert_equal [101, 104], github.posts
    assert_match(/101/, error.message)
  end

  def test_requires_two_current_write_access_approvals
    reviews = [
      FakeGitHub.review("alice", "APPROVED", "old", "2026-01-01T00:00:00Z"),
      FakeGitHub.review("mallory", "APPROVED", "head", "2026-01-01T00:00:01Z"),
      FakeGitHub.review("bob", "CHANGES_REQUESTED", "head", "2026-01-01T00:00:02Z")
    ]
    github = FakeGitHub.new(reviews: reviews, runs: [[FakeGitHub.run(101)]])

    controller(github).run

    assert_empty github.posts
    assert_equal 0, github.run_queries
  end

  def test_waits_for_a_stable_run_set_before_releasing
    first = [FakeGitHub.run(101)]
    complete = [FakeGitHub.run(101), FakeGitHub.run(102)]
    github = FakeGitHub.new(
      runs: [first, complete, complete],
      jobs: { 101 => FakeGitHub.gate_failure, 102 => FakeGitHub.gate_failure }
    )

    controller(github).run

    assert_equal 3, github.run_queries
    assert_equal [101, 102], github.posts
  end

  def test_ignores_runs_for_another_pull_request_with_the_same_head
    runs = [FakeGitHub.run(101), FakeGitHub.run(102, pr_number: 99)]
    github = FakeGitHub.new(
      runs: [runs, runs],
      jobs: { 101 => FakeGitHub.gate_failure, 102 => FakeGitHub.gate_failure }
    )

    controller(github).run

    assert_equal [101], github.posts
  end

  def test_aborts_if_approval_is_dismissed_before_post
    dismissed = [
      FakeGitHub.review("alice", "APPROVED", "head", "2026-01-01T00:00:00Z"),
      FakeGitHub.review("bob", "CHANGES_REQUESTED", "head", "2026-01-01T00:00:02Z")
    ]
    run = FakeGitHub.run(101)
    github = FakeGitHub.new(
      review_snapshots: [FakeGitHub.approved_reviews, dismissed],
      runs: [[run], [run]],
      jobs: { 101 => FakeGitHub.gate_failure }
    )

    error = assert_raises(DependabotCiRelease::Error) { controller(github).run }

    assert_match(/approvals changed before release/, error.message)
    assert_empty github.posts
  end
end
