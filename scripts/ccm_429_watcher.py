#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = ["tomli", "requests"]
# ///

import json
import signal
import subprocess
import sys
import threading
from datetime import datetime
from pathlib import Path

import tomli
import requests


CONFIG_PATH = Path.home() / ".claude-code-mux" / "config.toml"
CCM_BASE_URL = "http://127.0.0.1:13456"
RESTORE_INTERVAL_SECONDS = 15 * 60
COOLDOWN_SECONDS = 30
LINE_BUFFER_SIZE = 10


class ConfigManager:
    def __init__(self, config_path: Path):
        self.config_path = config_path
        self.original_config = ""
        self.current_config = ""
        self.disabled_mappings = []
        self.last_restore_time = datetime.now()
        self._lock = threading.Lock()

    def load_original(self):
        with open(self.config_path, "rb") as f:
            self.original_config = f.read().decode("utf-8")
        self.current_config = self.original_config
        print(f"[ConfigManager] Loaded from {self.config_path}")

    def restore_original(self):
        with self._lock:
            print(f"[ConfigManager] Restoring original...")
            with open(self.config_path, "w") as f:
                f.write(self.original_config)
            self.current_config = self.original_config
            self.disabled_mappings.clear()
            self.last_restore_time = datetime.now()
            self._call_reload()

    def disable_mapping(self, provider: str, actual_model: str) -> bool:
        with self._lock:
            if any(m[1] == actual_model for m in self.disabled_mappings):
                print(f"[ConfigManager] Already disabled: {actual_model}")
                return False

            config = tomli.loads(self.current_config)

            if "models" not in config:
                print(f"[ConfigManager] No models section")
                return False

            found = False
            for model in config.get("models", []):
                model_name = model.get("name", "")
                for mapping in model.get("mappings", []):
                    if mapping.get("actual_model") == actual_model:
                        found = True
                        print(
                            f"[ConfigManager] Disabling: {model_name} -> {actual_model} (provider={provider})"
                        )
                        self.disabled_mappings.append((model_name, actual_model))

            if not found:
                print(f"[ConfigManager] Not found: actual_model={actual_model}")
                return False

            self._comment_out_mappings(actual_model)
            self.last_restore_time = datetime.now()
            return True

    def _comment_out_mappings(self, actual_model: str):
        lines = self.current_config.split("\n")
        new_lines = []
        in_mapping_block = False
        comment_this_block = False

        for line in lines:
            stripped = line.strip()

            if stripped.startswith("[[models.mappings]]"):
                in_mapping_block = True
                comment_this_block = False
                new_lines.append(line)
                continue

            if in_mapping_block:
                if stripped.startswith("actual_model"):
                    if (
                        f'"{actual_model}"' in stripped
                        or f"'{actual_model}'" in stripped
                    ):
                        comment_this_block = True

                if comment_this_block:
                    if stripped.startswith("[[models.mappings]]"):
                        in_mapping_block = False
                        comment_this_block = False
                        new_lines.append(line)
                        continue
                    new_lines.append("# " + line)
                else:
                    if stripped.startswith("[[models]]"):
                        in_mapping_block = False

            new_lines.append(line)

        self.current_config = "\n".join(new_lines)

        with open(self.config_path, "w") as f:
            f.write(self.current_config)

        self._call_reload()
        print(f"[ConfigManager] Updated and reloaded")

    def _call_reload(self):
        try:
            r = requests.post(f"{CCM_BASE_URL}/api/reload", timeout=10)
            if r.status_code == 200:
                print(f"[ConfigManager] CCM reloaded")
            else:
                print(f"[ConfigManager] CCM reload status: {r.status_code}")
        except Exception as e:
            print(f"[ConfigManager] Reload failed: {e}")


class JournalWatcher:
    def __init__(self, config_manager: ConfigManager):
        self.config_manager = config_manager
        self.running = False
        self.thread = None
        self.last_429_time = None

    def start(self):
        self.running = True
        self.thread = threading.Thread(target=self._watch, daemon=True)
        self.thread.start()
        print("[JournalWatcher] Started")

    def stop(self):
        self.running = False
        if self.thread:
            self.thread.join(timeout=5)
        print("[JournalWatcher] Stopped")

    def _watch(self):
        process = subprocess.Popen(
            ["journalctl", "-f", "-u", "ccm", "-o", "json"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )

        line_buffer = []

        try:
            for line in process.stdout:
                if not self.running:
                    break

                try:
                    entry = json.loads(line)
                    message = entry.get("MESSAGE", "")
                except (json.JSONDecodeError, KeyError):
                    message = line.strip()

                if "429" in message and "error" in message.lower():
                    print(f"[JournalWatcher] 429 detected: {message[:100]}...")

                    provider = None
                    actual_model = None

                    for _, prev_line in reversed(line_buffer):
                        if not provider:
                            for part in prev_line.split():
                                if part.startswith("provider="):
                                    provider = part.split("=", 1)[1]
                                    break

                        if not actual_model:
                            for part in prev_line.split():
                                if part.startswith("actual_model="):
                                    actual_model = part.split("=", 1)[1]
                                    break

                        if provider and actual_model:
                            break

                    if provider and actual_model:
                        now = datetime.now()
                        if (
                            self.last_429_time
                            and (now - self.last_429_time).total_seconds()
                            < COOLDOWN_SECONDS
                        ):
                            print(f"[JournalWatcher] Cooldown active, skipping")
                            continue

                        self.last_429_time = now
                        print(
                            f"[JournalWatcher] Disabling: provider={provider}, actual_model={actual_model}"
                        )
                        self.config_manager.disable_mapping(provider, actual_model)
                    else:
                        print(f"[JournalWatcher] Could not extract context")

                line_buffer.append(message)
                if len(line_buffer) > LINE_BUFFER_SIZE:
                    line_buffer.pop(0)

        except Exception as e:
            print(f"[JournalWatcher] Error: {e}")
        finally:
            process.terminate()
            process.wait()


class RateLimitWatcher:
    def __init__(self):
        self.config_manager = ConfigManager(CONFIG_PATH)
        self.journal_watcher = JournalWatcher(self.config_manager)
        self.running = False
        self.restore_timer = None
        self._shutdown_event = threading.Event()

    def start(self):
        print("[Service] Starting CCM 429 Watcher")

        if not self.config_manager.config_path.exists():
            print(f"[Service] Config not found: {self.config_manager.config_path}")
            sys.exit(1)

        self.config_manager.load_original()

        signal.signal(signal.SIGINT, self._signal_handler)
        signal.signal(signal.SIGTERM, self._signal_handler)

        self.journal_watcher.start()
        self._schedule_restore()

        self.running = True
        print(f"[Service] Started")
        print(f"[Service] Config: {CONFIG_PATH}")
        print(f"[Service] CCM: {CCM_BASE_URL}")
        print(f"[Service] Restore interval: {RESTORE_INTERVAL_SECONDS // 60} min")

        self._shutdown_event.wait()

    def _schedule_restore(self):
        self.restore_timer = threading.Timer(
            RESTORE_INTERVAL_SECONDS, self._restore_callback
        )
        self.restore_timer.daemon = True
        self.restore_timer.start()

    def _restore_callback(self):
        if self.running:
            print("[Service] Scheduled restore")
            self.config_manager.restore_original()
            self._schedule_restore()

    def _signal_handler(self, signum, frame):
        print(f"[Service] Signal {signum}")
        self.shutdown()

    def shutdown(self):
        print("[Service] Shutting down...")
        self.running = False
        self._shutdown_event.set()

        self.journal_watcher.stop()

        if self.restore_timer:
            self.restore_timer.cancel()

        self.config_manager.restore_original()

        print("[Service] Done")


def main():
    service = RateLimitWatcher()
    service.start()


if __name__ == "__main__":
    main()
