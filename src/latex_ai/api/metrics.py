"""Metrics API endpoint for monitoring."""
from fastapi import APIRouter
from fastapi.responses import PlainTextResponse

from latex_ai.metrics import get_metrics

router = APIRouter()


@router.get("/api/metrics")
async def metrics_endpoint():
    """Return Prometheus-compatible metrics."""
    metrics = get_metrics()
    prometheus_text = metrics.to_prometheus()
    return PlainTextResponse(content=prometheus_text, media_type="text/plain; version=0.0.4")


@router.get("/api/metrics/json")
async def metrics_json_endpoint():
    """Return metrics in JSON format for easier debugging."""
    metrics = get_metrics()
    return metrics.to_dict()
