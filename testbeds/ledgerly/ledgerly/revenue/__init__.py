"""Revenue recognition for service periods (subscriptions)."""

from .recognition import build_schedule, recognise_period

__all__ = ["build_schedule", "recognise_period"]
