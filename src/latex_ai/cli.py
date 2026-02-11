from __future__ import annotations
import json
from pathlib import Path
import typer
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.markdown import Markdown

from .engine import LatexEngine
from .commands import CommandRegistry
from .document import Document
from .ai_planner import get_ai_planner

app = typer.Typer(help="Interactive LaTeX formatting engine")
console = Console()


def _ensure_file(path: Path) -> Path:
    p = Path(path)
    if not p.exists():
        typer.secho(f"File not found: {p}", fg=typer.colors.RED)
        raise typer.Exit(code=1)
    return p


@app.command()
def preview(file: Path):
    """Show a quick summary of key formatting settings."""
    p = _ensure_file(file)
    engine = LatexEngine(p)
    info = engine.preview()
    table = Table(title=str(p))
    table.add_column("Key")
    table.add_column("Value")
    for k, v in info.items():
        table.add_row(k, v)
    console.print(table)


@app.command()
def apply(file: Path, command: str, dry_run: bool = typer.Option(False, "--dry-run", "-n", help="Preview changes without saving")):
    """Apply a single command to the LaTeX file.

    Example: latex-ai apply paper.tex "set margins 1in"
    
    Use --dry-run to preview changes without modifying the file.
    """
    p = _ensure_file(file)
    try:
        engine = LatexEngine(p)
    except Exception as e:
        typer.secho(f"Error loading file: {e}", fg=typer.colors.RED)
        raise typer.Exit(code=1)
    
    result = engine.apply_command(command, auto_save=False)
    
    if not result.changed:
        typer.secho(result.message, fg=typer.colors.YELLOW)
        return
    
    # Show diff if changed
    if result.new_text:
        diff = engine.compute_diff(result.new_text)
        if diff:
            console.print("\n[bold cyan]Preview of changes:[/bold cyan]")
            console.print(diff)
            console.print()
    
    if dry_run:
        typer.secho(f"DRY RUN: {result.message} (file not modified)", fg=typer.colors.BLUE)
    else:
        # Apply changes
        try:
            engine.text = result.new_text
            backup_path = engine.save(backup=True)
            typer.secho(result.message, fg=typer.colors.GREEN)
            if backup_path:
                typer.secho(f"Backup created: {backup_path.name}", fg=typer.colors.BRIGHT_BLACK)
        except Exception as e:
            typer.secho(f"Error saving file: {e}", fg=typer.colors.RED)
            raise typer.Exit(code=1)


@app.command()
def repl(file: Path):
    """Interactive loop to try commands quickly.
    
    Each command shows a diff preview and saves immediately.
    The file is reloaded after each edit to maintain consistency.
    """
    p = _ensure_file(file)
    
    try:
        engine = LatexEngine(p)
    except Exception as e:
        typer.secho(f"Error loading file: {e}", fg=typer.colors.RED)
        raise typer.Exit(code=1)
    
    typer.echo("Enter commands (empty line to quit). Examples:")
    typer.echo("  set margins 1in")
    typer.echo("  set line spacing 1.5")
    typer.echo("  set figure width 0.9\\textwidth")
    typer.echo("  change documentclass to article")
    typer.echo("  add package hyperref")
    typer.echo("  remove package geometry")
    typer.echo("  normalize whitespace\n")
    
    while True:
        try:
            cmd = typer.prompt("latex-ai>")
        except (typer.Abort, EOFError, KeyboardInterrupt):
            typer.echo("\nExiting...")
            break
        
        cmd = cmd.strip()
        if not cmd:
            break
        
        try:
            result = engine.apply_command(cmd, auto_save=False)
            
            if not result.changed:
                typer.secho(result.message, fg=typer.colors.YELLOW)
                continue
            
            # Show diff preview
            if result.new_text:
                diff = engine.compute_diff(result.new_text)
                if diff:
                    console.print("\n[dim]" + diff[:500] + ("..." if len(diff) > 500 else "") + "[/dim]\n")
            
            # Save and reload
            engine.text = result.new_text
            backup_path = engine.save(backup=True)
            typer.secho(result.message, fg=typer.colors.GREEN)
            
            # CRITICAL: Reload file to fix state bug
            engine.reload()
            
        except IOError as e:
            typer.secho(f"I/O Error: {e}", fg=typer.colors.RED)
        except Exception as e:
            typer.secho(f"Unexpected error: {e}", fg=typer.colors.RED)


@app.command("list-commands")
def list_commands():
    """List all available LaTeX editing commands."""
    commands = CommandRegistry.list_commands()
    
    if not commands:
        typer.secho("No commands registered.", fg=typer.colors.YELLOW)
        return
    
    console.print("\n[bold cyan]Available LaTeX Commands[/bold cyan]\n")
    
    for cmd in sorted(commands, key=lambda c: c.name):
        # Create panel for each command
        content = f"""[bold]{cmd.description}[/bold]

[dim]Examples:[/dim]
"""
        for example in cmd.examples:
            content += f"  • latex-ai apply file.tex \"{example}\"\n"
        
        console.print(Panel(content.strip(), title=f"[green]{cmd.name}[/green]", border_style="blue"))
    
    console.print(f"\n[dim]Total: {len(commands)} commands available[/dim]\n")


@app.command()
def plan(file: Path, request: str, apply_plan: bool = typer.Option(False, "--apply", "-a", help="Apply the plan automatically")):
    """Generate an AI editing plan for a LaTeX document.
    
    Uses Gemini API to analyze document and suggest formatting changes.
    
    Example: latex-ai plan paper.tex "Make conference-ready"
    
    Use --apply to execute the plan automatically.
    Requires GEMINI_API_KEY environment variable.
    """
    p = _ensure_file(file)
    
    # Check for API key
    if not typer.confirm("This requires a Gemini API key. Do you have one set?"):
        typer.echo("Set GEMINI_API_KEY environment variable and try again.")
        raise typer.Exit(code=1)
    
    try:
        typer.secho("Loading document...", fg=typer.colors.BLUE)
        doc = Document(p)
        
        typer.secho("Generating plan with AI (this may take a moment)...", fg=typer.colors.BLUE)
        planner = get_ai_planner()
        plan_obj = planner.create_plan(doc, request)
        
        # Display plan
        console.print(Panel(
            f"[bold cyan]{plan_obj.goal}[/bold cyan]\n\n{plan_obj.summary}",
            title="[green]AI Editing Plan[/green]",
            border_style="blue"
        ))
        
        console.print(f"\n[bold cyan]Proposed Steps ({len(plan_obj.steps)} total):[/bold cyan]\n")
        
        for i, step in enumerate(plan_obj.steps, 1):
            console.print(f"[cyan]{i}.[/cyan] [bold]{step.command}[/bold]")
            console.print(f"   {step.description}")
            if step.rationale:
                console.print(f"   [dim]{step.rationale}[/dim]")
            console.print()
        
        console.print(f"[dim]Confidence: {plan_obj.confidence:.0%}[/dim]\n")
        
        if apply_plan or typer.confirm("Apply this plan?"):
            typer.secho("Applying plan...", fg=typer.colors.BLUE)
            engine = LatexEngine(p)
            
            for i, step in enumerate(plan_obj.steps, 1):
                try:
                    result = engine.apply_command(step.command, auto_save=False)
                    if result.changed:
                        typer.secho(f"  [OK] Step {i}: {step.command}", fg=typer.colors.GREEN)
                        engine.text = result.new_text
                        engine.save(backup=True)
                        engine.reload()
                    else:
                        typer.secho(f"  [EXCLUDED] Step {i}: {step.command} (no changes)", fg=typer.colors.YELLOW)
                except Exception as e:
                    typer.secho(f"  [FAIL] Step {i}: {step.command} - {e}", fg=typer.colors.RED)
            
            typer.secho("\nPlan applied successfully!", fg=typer.colors.GREEN)
        else:
            typer.secho("Plan not applied.", fg=typer.colors.YELLOW)
    
    except ValueError as e:
        typer.secho(f"AI Error: {e}", fg=typer.colors.RED)
        raise typer.Exit(code=1)
    except Exception as e:
        typer.secho(f"Error: {e}", fg=typer.colors.RED)
        raise typer.Exit(code=1)


@app.command()
def serve(
    port: int = typer.Option(3000, "--port", "-p", help="Port to serve on"),
    host: str = typer.Option("127.0.0.1", "--host", "-h", help="Host to bind to"),
    auto_open: bool = typer.Option(True, "--auto-open", help="Auto-open browser")
):
    """Start web server for LaTeX-AI UI.
    
    Launches FastAPI server on http://HOST:PORT and opens browser.
    
    Example: latex-ai serve --port 3000
    """
    import webbrowser
    import uvicorn
    from .server import app as fastapi_app
    
    try:
        url = f"http://{host}:{port}"
        
        typer.secho(f"Starting LaTeX-AI Server on {url}", fg=typer.colors.CYAN)
        
        if auto_open:
            # Open browser in background
            import threading
            def open_browser():
                import time
                time.sleep(1)  # Give server time to start
                webbrowser.open(url)
            
            thread = threading.Thread(target=open_browser, daemon=True)
            thread.start()
        
        # Start FastAPI server
        uvicorn.run(
            fastapi_app,
            host=host,
            port=port,
            log_level="info"
        )
    
    except KeyboardInterrupt:
        typer.secho("\nServer stopped", fg=typer.colors.YELLOW)
        raise typer.Exit(code=0)
    except Exception as e:
        typer.secho(f"Error starting server: {e}", fg=typer.colors.RED)
        raise typer.Exit(code=1)


@app.command()
def start(
    host: str = typer.Option("127.0.0.1", "--host", "-h", help="Server host"),
    port: int = typer.Option(3000, "--port", "-p", help="Server port"),
    dev: bool = typer.Option(False, "--dev", "-d", help="Development mode with auto-reload")
):
    """Start the full stack application (frontend + backend).
    
    In production mode, serves pre-built static frontend.
    In dev mode, runs backend with auto-reload (run 'npm run dev' separately for frontend).
    
    Examples:
        latex-ai start                    # Production mode on port 3000
        latex-ai start --port 8000        # Custom port
        latex-ai start --dev              # Development mode with reload
    """
    import uvicorn
    from .server import app as fastapi_app
    
    if dev:
        typer.secho("Starting in DEVELOPMENT mode (backend only)", fg=typer.colors.YELLOW)
        typer.secho("Run 'npm run dev' in frontend/ for live frontend", fg=typer.colors.BRIGHT_BLACK)
        typer.echo()
    else:
        # Check if static frontend is built
        static_dir = Path(__file__).parent.parent.parent / "frontend" / "dist"
        if not static_dir.exists() or not (static_dir / "index.html").exists():
            typer.secho("[ERROR] Frontend not built!", fg=typer.colors.RED)
            typer.secho("   Run 'npm run build' in frontend/ first", fg=typer.colors.YELLOW)
            typer.echo()
            typer.secho("   Or use --dev mode: latex-ai start --dev", fg=typer.colors.BRIGHT_BLACK)
            raise typer.Exit(1)
    
    try:
        url = f"http://{host}:{port}"
        typer.secho(f"[LAUNCH] LaTeX-AI Server starting on {url}", fg=typer.colors.GREEN)
        typer.secho(f"   Mode: {'Development' if dev else 'Production'}", fg=typer.colors.BRIGHT_BLACK)
        typer.echo()
        
        # Start FastAPI server
        uvicorn.run(
            "latex_ai.server:app",
            host=host,
            port=port,
            reload=dev,
            log_level="info" if dev else "warning"
        )
    
    except KeyboardInterrupt:
        typer.secho("\n[STOP] Server stopped", fg=typer.colors.YELLOW)
        raise typer.Exit(code=0)
    except Exception as e:
        typer.secho(f"[ERROR] Error starting server: {e}", fg=typer.colors.RED)
        raise typer.Exit(code=1)


if __name__ == "__main__":
    app()  # pragma: no cover
