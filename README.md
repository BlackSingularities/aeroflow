# AeroFlow — interaktywny tunel aerodynamiczny

Symulator dynamiki płynów 2D w czasie rzeczywistym: **Python (numba) liczy przepływ metodą Lattice-Boltzmanna**, a **frontend w React + TypeScript** wizualizuje go i pozwala rysować przeszkody — skrzydła NACA, dowolne kształty, ściany, koła, prostokąty.

## Uruchomienie

```bash
pip install -r requirements.txt
cd web && npm install && npm run build && cd ..
python server/run.py
```

Otworzy się http://127.0.0.1:8765. Tryb deweloperski frontendu (HMR): `python server/run.py --no-browser` oraz `cd web && npm run dev` → http://localhost:5173.

## Funkcje

- **Skrzydło NACA 4-cyfrowe** — przeciągnij od noska do krawędzi spływu (cięciwa + kąt natarcia), grubość i wygięcie regulowane suwakami.
- **Kształt** — narysuj dowolny obrys, zostanie wypełniony. Pędzel, gumka (lub prawy przycisk), koło, prostokąt.
- **Wizualizacja**: prędkość, wirowość, ciśnienie; cząstki-znaczniki (RK2) lub smugi dymu z grzebienia na wlocie.
- **Siły**: Cd, Cl, L/D liczone metodą wymiany pędu, wykres w czasie, liczba Reynoldsa, sonda pod kursorem (|u|/U∞, Cp).
- Scenariusze: profil, walec (ścieżka Kármána), tandem, płyta, krata, uskok.
- Skróty: `1–6` narzędzia, `spacja` pauza, `V` widok, `C` wyczyść, `R` reset przepływu.

## Wydajność

- Solver D2Q9 w jednym kernelu **numba `@njit(parallel=True, fastmath=True)`**: pull-streaming + half-way bounce-back + kolizja BGK w jednym przejściu po pamięci, równolegle po wierszach — ok. **150–230 MLUPS** na CPU (≈30× szybciej niż wektoryzowany numpy, który zostaje jako fallback bez numby).
- Model turbulencji **Smagorinsky LES** (tensor naprężeń z momentów, bez dodatkowych przebiegów) + strefa gąbki na wylocie — stabilny przy wysokich Re.
- Symulacja działa w osobnym wątku z precyzyjnym taktowaniem 60 Hz; WebSocket zawsze wysyła najświeższą klatkę (wolny klient gubi klatki zamiast budować kolejkę).
- Binarny protokół: pola jako `int16`, maska ciał upakowana bitowo.
- Frontend: kolory przez LUT, pole renderowane w rozdzielczości siatki i skalowane na GPU, cząstki rysowane jedną ścieżką na klatkę.

## Struktura

```
server/lbm.py   solver LBM (numba + fallback numpy), geometria NACA
server/app.py   FastAPI + WebSocket, protokół binarny, scenariusze
server/run.py   start serwera (z rozgrzewką JIT)
web/src/        React: App.tsx (UI, narzędzia), render.ts (mapy kolorów, cząstki), sim.ts (protokół)
```
