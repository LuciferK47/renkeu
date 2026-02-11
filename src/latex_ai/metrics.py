"""Prometheus metrics for LaTeX-AI server.

Provides a /api/metrics endpoint with usage statistics and performance data.
"""

import time
from threading import Lock
from typing import Dict, Any
from collections import defaultdict


class Metrics:
    """Prometheus-compatible metrics collector."""
    
    def __init__(self):
        self._lock = Lock()
        
        # Counters
        self.requests_total = defaultdict(int)  # {method: count}
        self.requests_success = defaultdict(int)  # {method: count}
        self.requests_failed = defaultdict(int)  # {method: count}
        
        self.agent_requests_total = 0
        self.agent_requests_success = 0
        self.agent_requests_failed = 0
        
        self.compilations_total = 0
        self.compilations_success = 0
        self.compilations_failed = 0
        
        # Histograms (track durations)
        self.request_duration_ms = []  # Sample of durations
        self.agent_duration_ms = []
        self.compile_duration_ms = []
        
        self._max_samples = 100  # Keep last N samples
    
    def record_request(self, method: str, success: bool, duration_ms: float) -> None:
        """Record HTTP request metric.
        
        Args:
            method: HTTP method (GET, POST, etc.)
            success: Whether request succeeded
            duration_ms: Request duration in milliseconds
        """
        with self._lock:
            self.requests_total[method] += 1
            if success:
                self.requests_success[method] += 1
            else:
                self.requests_failed[method] += 1
            
            self.request_duration_ms.append(duration_ms)
            if len(self.request_duration_ms) > self._max_samples:
                self.request_duration_ms.pop(0)
    
    def record_agent_request(self, success: bool, duration_ms: float) -> None:
        """Record agent request metric.
        
        Args:
            success: Whether agent succeeded
            duration_ms: Duration in milliseconds
        """
        with self._lock:
            self.agent_requests_total += 1
            if success:
                self.agent_requests_success += 1
            else:
                self.agent_requests_failed += 1
            
            self.agent_duration_ms.append(duration_ms)
            if len(self.agent_duration_ms) > self._max_samples:
                self.agent_duration_ms.pop(0)
    
    def record_compilation(self, success: bool, duration_ms: float) -> None:
        """Record LaTeX compilation metric.
        
        Args:
            success: Whether compilation succeeded
            duration_ms: Duration in milliseconds
        """
        with self._lock:
            self.compilations_total += 1
            if success:
                self.compilations_success += 1
            else:
                self.compilations_failed += 1
            
            self.compile_duration_ms.append(duration_ms)
            if len(self.compile_duration_ms) > self._max_samples:
                self.compile_duration_ms.pop(0)
    
    def get_prometheus_text(self) -> str:
        """Return metrics in Prometheus text format.
        
        Returns:
            Prometheus-compatible metrics text
        """
        with self._lock:
            lines = []
            
            # TYPE and HELP declarations
            lines.append("# HELP latex_ai_requests_total Total HTTP requests")
            lines.append("# TYPE latex_ai_requests_total counter")
            
            # Request counters
            for method, count in self.requests_total.items():
                lines.append(f'latex_ai_requests_total{{method="{method}"}} {count}')
            
            lines.append("# HELP latex_ai_requests_success Successful HTTP requests")
            lines.append("# TYPE latex_ai_requests_success counter")
            for method, count in self.requests_success.items():
                lines.append(f'latex_ai_requests_success{{method="{method}"}} {count}')
            
            lines.append("# HELP latex_ai_requests_failed Failed HTTP requests")
            lines.append("# TYPE latex_ai_requests_failed counter")
            for method, count in self.requests_failed.items():
                lines.append(f'latex_ai_requests_failed{{method="{method}"}} {count}')
            
            # Agent metrics
            lines.append("# HELP latex_ai_agent_requests_total Total agent requests")
            lines.append("# TYPE latex_ai_agent_requests_total counter")
            lines.append(f"latex_ai_agent_requests_total {self.agent_requests_total}")
            
            lines.append("# HELP latex_ai_agent_requests_success Successful agent requests")
            lines.append("# TYPE latex_ai_agent_requests_success counter")
            lines.append(f"latex_ai_agent_requests_success {self.agent_requests_success}")
            
            lines.append("# HELP latex_ai_agent_requests_failed Failed agent requests")
            lines.append("# TYPE latex_ai_agent_requests_failed counter")
            lines.append(f"latex_ai_agent_requests_failed {self.agent_requests_failed}")
            
            # Compilation metrics
            lines.append("# HELP latex_ai_compilations_total Total LaTeX compilations")
            lines.append("# TYPE latex_ai_compilations_total counter")
            lines.append(f"latex_ai_compilations_total {self.compilations_total}")
            
            lines.append("# HELP latex_ai_compilations_success Successful compilations")
            lines.append("# TYPE latex_ai_compilations_success counter")
            lines.append(f"latex_ai_compilations_success {self.compilations_success}")
            
            lines.append("# HELP latex_ai_compilations_failed Failed compilations")
            lines.append("# TYPE latex_ai_compilations_failed counter")
            lines.append(f"latex_ai_compilations_failed {self.compilations_failed}")
            
            # Duration histograms
            if self.request_duration_ms:
                avg_request = sum(self.request_duration_ms) / len(self.request_duration_ms)
                lines.append("# HELP latex_ai_request_duration_ms Request duration in milliseconds")
                lines.append("# TYPE latex_ai_request_duration_ms gauge")
                lines.append(f"latex_ai_request_duration_ms {avg_request:.2f}")
            
            if self.agent_duration_ms:
                avg_agent = sum(self.agent_duration_ms) / len(self.agent_duration_ms)
                lines.append("# HELP latex_ai_agent_duration_ms Agent request duration in milliseconds")
                lines.append("# TYPE latex_ai_agent_duration_ms gauge")
                lines.append(f"latex_ai_agent_duration_ms {avg_agent:.2f}")
            
            if self.compile_duration_ms:
                avg_compile = sum(self.compile_duration_ms) / len(self.compile_duration_ms)
                lines.append("# HELP latex_ai_compile_duration_ms Compilation duration in milliseconds")
                lines.append("# TYPE latex_ai_compile_duration_ms gauge")
                lines.append(f"latex_ai_compile_duration_ms {avg_compile:.2f}")
            
            return "\n".join(lines) + "\n"
    
    def get_summary(self) -> Dict[str, Any]:
        """Get summary statistics as JSON.
        
        Returns:
            Dictionary with metrics summary
        """
        with self._lock:
            request_success_rate = 0.0
            if self.requests_total:
                total_reqs = sum(self.requests_total.values())
                success_reqs = sum(self.requests_success.values())
                request_success_rate = (success_reqs / total_reqs * 100) if total_reqs > 0 else 0.0
            
            agent_success_rate = 0.0
            if self.agent_requests_total > 0:
                agent_success_rate = (self.agent_requests_success / self.agent_requests_total * 100)
            
            compile_success_rate = 0.0
            if self.compilations_total > 0:
                compile_success_rate = (self.compilations_success / self.compilations_total * 100)
            
            return {
                "requests": {
                    "total": sum(self.requests_total.values()),
                    "success": sum(self.requests_success.values()),
                    "failed": sum(self.requests_failed.values()),
                    "success_rate_percent": round(request_success_rate, 2),
                    "by_method": dict(self.requests_total),
                },
                "agent": {
                    "total": self.agent_requests_total,
                    "success": self.agent_requests_success,
                    "failed": self.agent_requests_failed,
                    "success_rate_percent": round(agent_success_rate, 2),
                    "avg_duration_ms": (
                        round(sum(self.agent_duration_ms) / len(self.agent_duration_ms), 2)
                        if self.agent_duration_ms else 0
                    ),
                },
                "compilation": {
                    "total": self.compilations_total,
                    "success": self.compilations_success,
                    "failed": self.compilations_failed,
                    "success_rate_percent": round(compile_success_rate, 2),
                    "avg_duration_ms": (
                        round(sum(self.compile_duration_ms) / len(self.compile_duration_ms), 2)
                        if self.compile_duration_ms else 0
                    ),
                },
            }


# Global metrics instance
_metrics_instance = None


def get_metrics() -> Metrics:
    """Get or create global metrics instance."""
    global _metrics_instance
    if _metrics_instance is None:
        _metrics_instance = Metrics()
    return _metrics_instance
