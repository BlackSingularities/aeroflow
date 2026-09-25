"""Uruchamia serwer AeroFlow (backend + zbudowany frontend) na http://127.0.0.1:8765."""
import os
import sys
import webbrowser

import uvicorn

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

if __name__ == "__main__":
    # rozgrzewka JIT, żeby pierwsza klatka nie czekała na kompilację numby
    from lbm import Simulation

    Simulation(64, 32).step(1)
    if "--no-browser" not in sys.argv:
        webbrowser.open("http://127.0.0.1:8765")
    uvicorn.run("app:app", host="127.0.0.1", port=8765, app_dir=os.path.dirname(os.path.abspath(__file__)))
