# Multi-stage Dockerfile for LaTeX-AI production deployment

# Stage 1: Build frontend
FROM node:18-alpine AS frontend-builder

WORKDIR /app/frontend

# Copy package files and install dependencies
COPY frontend/package*.json ./
RUN npm ci --production=false

# Copy frontend source and build
COPY frontend/ ./
RUN npm run build

# Stage 2: Python runtime with LaTeX
FROM python:3.11-slim

LABEL maintainer="Dhirendra"
LABEL description="LaTeX-AI IDE with AI-powered editing"

WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y \
    # LaTeX distribution
    texlive-latex-base \
    texlive-latex-extra \
    texlive-fonts-recommended \
    texlive-fonts-extra \
    texlive-bibtex-extra \
    # Additional utilities
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy Python dependencies file
COPY pyproject.toml ./

# Install Python dependencies
RUN pip install --no-cache-dir --upgrade pip && \
    pip install --no-cache-dir -e .

# Copy backend source code
COPY src/ ./src/

# Copy built frontend from stage 1
COPY --from=frontend-builder /app/frontend/dist ./src/latex_ai/static

# Create directories for data persistence
RUN mkdir -p /app/data /app/.latex-ai/compile_cache

# Environment variables
ENV LATEX_AI_HOST=0.0.0.0
ENV LATEX_AI_PORT=8000
ENV LATEX_AI_CACHE_DIR=/app/.latex-ai/compile_cache
ENV LATEX_AI_LOG_LEVEL=info

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8000/api/health || exit 1

# Expose port
EXPOSE 8000

# Run as non-root user
RUN useradd -m -u 1000 latexai && \
    chown -R latexai:latexai /app
USER latexai

# Start server with uvicorn
CMD ["uvicorn", "latex_ai.server:app", "--host", "0.0.0.0", "--port", "8000", "--workers", "4"]
