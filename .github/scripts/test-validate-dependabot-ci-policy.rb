#!/usr/bin/env ruby

require "minitest/autorun"
require_relative "validate-dependabot-ci-policy"

class DependabotCiPolicyValidatorTest < Minitest::Test
  def test_detects_job_status_functions_that_can_bypass_failed_dependencies
    [
      "always()",
      "failure() && needs.build.result == 'failure'",
      "cancelled()",
      "!cancelled()",
      "!success()",
      "success() == false"
    ].each do |condition|
      assert overrides_implicit_success?(condition), condition
    end
  end

  def test_accepts_conditions_that_do_not_override_implicit_success
    refute overrides_implicit_success?("needs.changes.outputs.rust == 'true'")
  end
end
