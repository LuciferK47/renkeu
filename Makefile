# LaTeX-AI Makefile
# Provides common operations for development and deployment

.PHONY: help clean clean-all pack install dev test lint format run

help:
	@echo "LaTeX-AI Project Commands:"
	@echo ""
	@echo "  make clean       - Remove build artifacts and caches"
	@echo "  make clean-all   - Remove everything including venv/node_modules"
	@echo "  make pack        - Create clean distribution ZIP"
	@echo "  make install     - Install Python dependencies"
	@echo "  make dev         - Install development dependencies"
	@echo "  make test        - Run test suite"
	@echo "  make lint        - Run linter (ruff)"
	@echo "  make format      - Format code (black)"
	@echo "  make run         - Start development server"
	@echo ""

# Remove build artifacts and caches (keeps dependencies)
clean:
	@echo "Cleaning build artifacts and caches..."
	rm -rf __pycache__ .pytest_cache .mypy_cache .ruff_cache
	rm -rf build dist *.egg-info
	rm -rf .compile_cache
	rm -rf htmlcov .coverage
	rm -rf frontend/.vite frontend/dist
	find . -type d -name "__pycache__" -exec rm -rf {} + 2>/dev/null || true
	find . -type f -name "*.pyc" -delete
	find . -type f -name "*.pyo" -delete
	find . -type f -name "*.log" -delete
	@echo "Clean complete!"

# Remove everything including dependencies (nuclear clean)
clean-all: clean
	@echo "Removing dependencies..."
	rm -rf .venv venv env
	rm -rf frontend/node_modules
	@echo "Nuclear clean complete! Run 'make install' to reinstall."

# Create distribution package
pack: clean
	@echo "Creating distribution package..."
	python package.py latex-ai-distribution.zip
	@echo "Package created: latex-ai-distribution.zip"

# Install Python dependencies
install:
	@echo "Installing Python dependencies..."
	pip install -e .
	@echo "Installing frontend dependencies..."
	cd frontend && npm install
	@echo "Installation complete!"

# Install development dependencies
dev: install
	@echo "Installing development dependencies..."
	pip install -e ".[dev]"
	@echo "Development setup complete!"

# Run test suite
test:
	@echo "Running test suite..."
	pytest tests/ -v --cov=src/latex_ai --cov-report=html

# Run linter
lint:
	@echo "Running ruff linter..."
	ruff check src/ tests/

# Format code
format:
	@echo "Formatting code with black..."
	black src/ tests/
	@echo "Formatting complete!"

# Start development server
run:
	@echo "Starting LaTeX-AI development server..."
	latex-ai serve --reload
