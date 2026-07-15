#!/usr/bin/env ruby

require "cgi"
require "json"
require "open3"

class GitHubClient
  class Error < StandardError; end

  def initialize(token: ENV.fetch("GH_TOKEN"))
    @token = token
  end

  def get(path)
    JSON.parse(run("api", path))
  end

  def pages(path)
    JSON.parse(run("api", "--paginate", "--slurp", path))
  end

  def post(path)
    run("api", "--method", "POST", path)
  end

  private

  def run(*arguments)
    output, status = Open3.capture2e({ "GH_TOKEN" => @token }, "gh", *arguments)
    raise Error, output.strip unless status.success?

    output
  end
end

class DependabotCiRelease
  class Error < StandardError; end

  GATE_JOB = "automation-approved / Automated PR CI approved"
  WRITE_PERMISSIONS = %w[write admin].freeze

  def initialize(github:, repository:, pr_number:, review_sha:, required_approvals: 2,
                 max_wait_attempts: 12, wait_seconds: 10, sleeper: Kernel.method(:sleep))
    @github = github
    @repository = repository
    @pr_number = pr_number
    @review_sha = review_sha
    @required_approvals = required_approvals
    @max_wait_attempts = max_wait_attempts
    @wait_seconds = wait_seconds
    @sleeper = sleeper
  end

  def run
    pull_request = current_pull_request!
    unless pull_request.dig("user", "login") == "dependabot[bot]"
      raise Error, "PR ##{@pr_number} is not authored by Dependabot"
    end

    approvals = current_write_approvals
    puts "Current-revision write-access approvals: #{approvals}/#{@required_approvals}"
    return if approvals < @required_approvals

    failures = []
    blocked_runs.each do |workflow_run|
      next unless gate_failed?(workflow_run.fetch("id"))

      run_id = workflow_run.fetch("id")
      puts "Releasing gate-blocked workflow run #{run_id}"
      # Authorization failures abort the release immediately. Only isolated API
      # failures while rerunning a still-authorized run are aggregated.
      current_pull_request!
      ensure_approved!
      begin
        @github.post("repos/#{@repository}/actions/runs/#{run_id}/rerun-failed-jobs")
      rescue StandardError => error
        warn "Failed to release workflow run #{run_id}: #{error.message}"
        failures << run_id
      end
    end

    return if failures.empty?

    raise Error, "failed to release workflow runs: #{failures.join(', ')}"
  end

  private

  def current_pull_request!
    pull_request = @github.get("repos/#{@repository}/pulls/#{@pr_number}")
    current_sha = pull_request.dig("head", "sha")
    return pull_request if current_sha == @review_sha

    raise Error, "PR head changed from #{@review_sha} to #{current_sha}; refusing to release stale CI"
  end

  def current_write_approvals
    reviews = paged_items(
      "repos/#{@repository}/pulls/#{@pr_number}/reviews?per_page=100"
    )
    latest_by_reviewer = {}
    reviews
      .select { |review| review["commit_id"] == @review_sha }
      .sort_by { |review| review["submitted_at"] }
      .each { |review| latest_by_reviewer[review.dig("user", "login")] = review }

    latest_by_reviewer.values.count do |review|
      next false unless review["state"] == "APPROVED"

      write_access?(review.dig("user", "login"))
    end
  end

  def ensure_approved!
    approvals = current_write_approvals
    return if approvals >= @required_approvals

    raise Error,
          "approvals changed before release: #{approvals}/#{@required_approvals} current-revision write-access approvals"
  end

  def write_access?(reviewer)
    permission = @github.get(
      "repos/#{@repository}/collaborators/#{CGI.escape(reviewer)}/permission"
    ).fetch("permission")
    allowed = WRITE_PERMISSIONS.include?(permission)
    puts "#{allowed ? 'Counting' : 'Ignoring'} approval from #{reviewer} (#{permission} access)"
    allowed
  rescue StandardError => error
    warn "Unable to verify repository access for #{reviewer}; ignoring approval: #{error.message}"
    false
  end

  def blocked_runs
    previous_settled_ids = nil
    @max_wait_attempts.times do |attempt|
      current_pull_request!
      latest_runs = latest_workflow_runs
      run_ids = latest_runs.map { |run| run.fetch("id") }.sort
      pending = latest_runs.count do |run|
        run.fetch("run_attempt", 0) == 1 && run["status"] != "completed"
      end
      stable = pending.zero? && !run_ids.empty? && run_ids == previous_settled_ids
      if stable
        return latest_runs.select do |run|
          run.fetch("run_attempt", 0) == 1 &&
            run["status"] == "completed" &&
            run["conclusion"] == "failure"
        end
      end

      if attempt == @max_wait_attempts - 1
        raise Error,
              "timed out waiting for a stable set of settled first-attempt workflow runs"
      end

      previous_settled_ids = pending.zero? ? run_ids : nil
      puts "Waiting for workflow runs to settle and stabilize " \
           "(#{pending} pending, attempt #{attempt + 1}/#{@max_wait_attempts})"
      @sleeper.call(@wait_seconds)
    end
  end

  def latest_workflow_runs
    query = "event=pull_request&head_sha=#{CGI.escape(@review_sha)}&per_page=100"
    runs = paged_objects(
      "repos/#{@repository}/actions/runs?#{query}", "workflow_runs"
    )
    runs
      .select do |run|
        Array(run["pull_requests"]).any? do |pull_request|
          pull_request["number"] == @pr_number
        end
      end
      .group_by { |run| run.fetch("workflow_id") }
      .values
      .map { |group| group.max_by { |run| run.fetch("created_at") } }
  end

  def gate_failed?(run_id)
    jobs = paged_objects(
      "repos/#{@repository}/actions/runs/#{run_id}/attempts/1/jobs?per_page=100", "jobs"
    )
    jobs.any? do |job|
      job["name"] == GATE_JOB && job["conclusion"] == "failure"
    end
  end

  def paged_items(path)
    @github.pages(path).flatten(1)
  end

  def paged_objects(path, key)
    @github.pages(path).flat_map { |page| page.fetch(key) }
  end
end

if $PROGRAM_NAME == __FILE__
  begin
    DependabotCiRelease.new(
      github: GitHubClient.new,
      repository: ENV.fetch("GITHUB_REPOSITORY"),
      pr_number: Integer(ENV.fetch("PR_NUMBER")),
      review_sha: ENV.fetch("REVIEW_SHA"),
      required_approvals: Integer(ENV.fetch("REQUIRED_APPROVALS", "2")),
      max_wait_attempts: Integer(ENV.fetch("MAX_WAIT_ATTEMPTS", "12")),
      wait_seconds: Integer(ENV.fetch("WAIT_SECONDS", "10"))
    ).run
  rescue StandardError => error
    warn "::error::#{error.message}"
    exit 1
  end
end
