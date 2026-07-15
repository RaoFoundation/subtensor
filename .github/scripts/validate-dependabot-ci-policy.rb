#!/usr/bin/env ruby

require "yaml"

GATE_JOB = "automation-approved"
GATE_WORKFLOW = "./.github/workflows/dependabot-ci-approval.yml"
POLICY_PATH = ".github/workflows/validate-dependabot-ci-policy.yml"
BOOTSTRAP_SENTINEL = [".github/workflows/typescript-e2e.yml", "typescript-formatting"].freeze
JOB_STATUS_FUNCTIONS = %w[always failure cancelled success].freeze

def workflow(path)
  YAML.safe_load(File.read(path), aliases: true)
end

def pull_request_workflow?(document)
  triggers = document["on"] || document[true]
  return true if triggers == "pull_request"
  return triggers.include?("pull_request") if triggers.is_a?(Array)

  triggers.is_a?(Hash) && triggers.key?("pull_request")
end

def reaches_gate?(jobs, job_name, seen = [])
  return true if job_name == GATE_JOB
  return false if seen.include?(job_name)

  dependencies = Array(jobs.fetch(job_name).fetch("needs", []))
  dependencies.any? do |dependency|
    reaches_gate?(jobs, dependency, seen + [job_name])
  end
end

def overrides_implicit_success?(condition)
  JOB_STATUS_FUNCTIONS.any? do |function|
    condition.to_s.match?(/\b#{Regexp.escape(function)}\s*\(\s*\)/)
  end
end

def validate_repository!
  workflow_files = Dir[".github/workflows/*.{yml,yaml}"].sort
  pull_request_workflows = workflow_files.select do |path|
    pull_request_workflow?(workflow(path))
  end

  pull_request_workflows.each do |path|
    jobs = workflow(path).fetch("jobs")
    gate = jobs[GATE_JOB]
    raise "#{path}: missing #{GATE_JOB} job" unless gate
    unless gate["uses"] == GATE_WORKFLOW
      raise "#{path}: #{GATE_JOB} must call #{GATE_WORKFLOW}"
    end

    ungated = jobs.keys.reject { |job_name| reaches_gate?(jobs, job_name) }
    unless ungated.empty?
      raise "#{path}: jobs without an approval dependency: #{ungated.join(', ')}"
    end
    jobs.each do |job_name, job|
      next unless overrides_implicit_success?(job.fetch("if", ""))
      next if path == POLICY_PATH && job_name == "validate-policy"
      next if [path, job_name] == BOOTSTRAP_SENTINEL

      dependencies = Array(job.fetch("needs", []))
      condition = job.fetch("if").to_s
      unless dependencies.include?(GATE_JOB) &&
             condition.include?("needs.#{GATE_JOB}.result == 'success'")
        raise "#{path}: #{job_name} overrides implicit success without directly enforcing #{GATE_JOB} success"
      end
    end
  end

  policy_job = workflow(POLICY_PATH).fetch("jobs").fetch("validate-policy")
  unless policy_job["name"] == "Dependabot CI policy" && policy_job["if"] == "always()"
    raise "#{POLICY_PATH}: dedicated policy check must always report a result"
  end
  unless policy_job.fetch("steps").any? { |step| step["name"] == "Enforce Dependabot CI approval" }
    raise "#{POLICY_PATH}: dedicated policy check no longer enforces approval"
  end

  typescript = workflow(".github/workflows/typescript-e2e.yml")
    .fetch("jobs").fetch("typescript-formatting")
  bootstrap_guard = typescript.fetch("steps").find do |step|
    step["name"] == "Enforce automated-PR approval"
  end
  unless typescript["if"] == "always()" &&
         bootstrap_guard&.fetch("if", "") == "needs.automation-approved.result != 'success'"
    raise ".github/workflows/typescript-e2e.yml: required-check bootstrap guard is missing"
  end

  approval = workflow(".github/workflows/approve-dependabot-ci.yml")
  approval_steps = approval.fetch("jobs").fetch("approve").fetch("steps")
  release_step = approval_steps.find { |step| step["name"] == "Release approved CI" }
  unless release_step && release_step["run"] == "ruby .github/scripts/release-dependabot-ci.rb"
    raise ".github/workflows/approve-dependabot-ci.yml: release must use the canonical script"
  end

  concurrency_group = approval.fetch("concurrency").fetch("group")
  unless concurrency_group.include?("github.run_id") &&
         concurrency_group.include?("github.event.review.commit_id")
    raise ".github/workflows/approve-dependabot-ci.yml: non-approval reviews must use unique concurrency groups"
  end

  puts "Dependabot CI policy: #{pull_request_workflows.length} PR workflows fully gated"
end

validate_repository! if $PROGRAM_NAME == __FILE__
