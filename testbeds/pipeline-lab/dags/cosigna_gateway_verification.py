"""Optional batch verification. This DAG never schedules customer transactions."""
from datetime import datetime, timedelta, timezone

from airflow.sdk import dag, get_current_context, task
from verification_runner import run_suite, summarize


@dag(dag_id="cosigna_gateway_verification", schedule=None,
     start_date=datetime(2026, 10, 23, tzinfo=timezone.utc), catchup=False,
     max_active_runs=1, default_args={"retries": 0}, tags=["cosigna", "fixtures"])
def gateway_verification():
    @task(execution_timeout=timedelta(minutes=10))
    def check(suite):
        run_suite(suite, get_current_context()["run_id"])

    @task(trigger_rule="all_done")
    def report():
        # A missing/failed upstream report fails this leaf too, so a successful
        # summary task cannot mask an upstream failure in the DAG's final state.
        summarize(get_current_context()["run_id"])

    crash = check.override(task_id="process_crash_recovery")("crash")
    rules = check.override(task_id="authorization_budget_and_replay")("rules")
    service = check.override(task_id="api_worker_restart")("service")
    summary = report()
    crash >> rules >> service
    [crash, rules, service] >> summary


gateway_verification()
