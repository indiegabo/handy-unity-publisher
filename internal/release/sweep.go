package release

import (
	"context"

	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
)

// PollSweepFailure records one trigger rule that could not be evaluated during
// a batch poll run.
type PollSweepFailure struct {
	TriggerRuleID int64  `json:"trigger_rule_id"`
	RepositoryID  int64  `json:"repository_id"`
	Error         string `json:"error"`
}

// PollSweepReport summarizes one deterministic batch evaluation over enabled
// poll trigger rules.
type PollSweepReport struct {
	Evaluated int                `json:"evaluated"`
	Results   []PollResult       `json:"results"`
	Failures  []PollSweepFailure `json:"failures,omitempty"`
}

// HasFailures reports whether any trigger rule failed during the sweep.
func (r PollSweepReport) HasFailures() bool {
	return len(r.Failures) > 0
}

// PollSweep runs a one-shot evaluation of every enabled poll trigger rule.
type PollSweep struct {
	triggers trigger.Store
	poller   *Poller
}

// NewPollSweep creates a one-shot scheduler over the trigger store and shared
// poller implementation.
func NewPollSweep(triggers trigger.Store, poller *Poller) *PollSweep {
	return &PollSweep{triggers: triggers, poller: poller}
}

// RunOnce evaluates enabled poll rules in rule id order, keeps going after
// per-rule failures, and returns a structured report for automation callers.
func (s *PollSweep) RunOnce(ctx context.Context) (PollSweepReport, error) {
	rules, err := s.triggers.ListEnabledBySource(ctx, trigger.SourcePoll)
	if err != nil {
		return PollSweepReport{}, err
	}

	report := PollSweepReport{
		Evaluated: len(rules),
		Results:   make([]PollResult, 0, len(rules)),
		Failures:  make([]PollSweepFailure, 0),
	}

	for _, rule := range rules {
		result, err := s.poller.PollRule(ctx, rule.ID)
		if err != nil {
			report.Failures = append(report.Failures, PollSweepFailure{
				TriggerRuleID: rule.ID,
				RepositoryID:  rule.RepositoryID,
				Error:         err.Error(),
			})
			continue
		}

		report.Results = append(report.Results, result)
	}

	return report, nil
}