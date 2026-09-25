# AeroFlow — interaktywny tunel aerodynamiczny

Symulator dynamiki płynów 2D w czasie rzeczywistym: **Python (numba) liczy przepływ metodą Lattice-Boltzmanna**, a **frontend w React + TypeScript** wizualizuje go i pozwala rysować przeszkody — skrzydła NACA, dowolne kształty, ściany, koła, prostokąty.

## Uruchomienie

```bash
pip install -r requirements.txt
cd web && npm install && npm run build && cd ..
python server/run.py
```

Otworzy się http://127.0.0.1:8765. Tryb deweloperski frontendu (HMR): `python server/run.py --no-browser` oraz `cd web && npm run dev` → http://localhost:5173.

## Interfejs

Symulacja zajmuje cały ekran (siatka dopasowuje się do proporcji okna), a sterowanie pływa nad nią:
dok narzędzi po lewej z panelem opcji, dolny dok (odtwarzanie, pole, znaczniki, scenariusze, wykres, ustawienia),
przeciągane i zwijane okna **Ustawienia** (Przepływ / Wygląd / Solver), **Wykres sił** i **Skróty**. Pozycje okien i preferencje są zapamiętywane.
`H` ukrywa cały interfejs, `F` włącza pełny ekran.

## Funkcje

- **Edycja obiektów** — każdy kształt to osobny obiekt. Narzędzie *Zaznacz* (`1`) przesuwa go przeciągnięciem, kółko myszy obraca (Shift = precyzyjnie), strzałki przesuwają, `Del` usuwa, `Ctrl+D` duplikuje.
  **Prawy przycisk myszy** na obiekcie otwiera menu: kąt natarcia/obrót (suwak i ±1°/±5°), grubość i wygięcie profilu, duplikuj, odbij, wyśrodkuj, usuń.
- **Cofnij / ponów** (`Ctrl+Z` / `Ctrl+Y`) dla wszystkich zmian geometrii.
- **Pole dymu** liczone na serwerze (adwekcja semi-Lagrange'owska), **linie prądu**, **wektory sił** (opór i siła nośna) rysowane na ciele, **zrzut ekranu** do PNG (`K`).

- **Skrzydło NACA 4-cyfrowe** (`2`) — przeciągnij od noska do krawędzi spływu (cięciwa + kąt natarcia), grubość i wygięcie regulowane suwakami.
- **Kształt** — narysuj dowolny obrys, zostanie wypełniony. Pędzel, gumka (lub prawy przycisk), koło, prostokąt.
- **Wizualizacja**: prędkość, wirowość, ciśnienie; cząstki-znaczniki (RK2) lub smugi dymu z grzebienia na wlocie.
- **Siły**: Cd, Cl, L/D liczone metodą wymiany pędu, wykres w czasie, liczba Reynoldsa, sonda pod kursorem (|u|/U∞, Cp).
- Scenariusze: profil, walec (ścieżka Kármána), tandem, płyta, krata, uskok.
- Skróty: `1–7` narzędzia, `PPM` opcje obiektu, `spacja` pauza, `V` pole, `P` znaczniki, `S` ustawienia, `G` wykres, `H` ukryj UI, `F` pełny ekran, `C` wyczyść, `R` reset, `Esc` zamknij okna.

## Wydajność

- Solver D2Q9 w jednym kernelu **numba `@njit(parallel=True, fastmath=True)`**: pull-streaming + half-way bounce-back + kolizja BGK w jednym przejściu po pamięci, równolegle po wierszach — ok. **150–230 MLUPS** na CPU (≈30× szybciej niż wektoryzowany numpy).
- Model turbulencji **Smagorinsky LES** (tensor naprężeń z momentów, bez dodatkowych przebiegów). Brzegi: wlot prędkościowy z ekstrapolowaną gęstością, ściany poślizgowe u góry i dołu, strefa gąbki relaksująca do strumienia swobodnego na wylocie — bez odbić fal i wirów.
- Gładkie kontury ciał liczone tylko przy zmianie kształtu (rozmyta maska + próg), poświata wypiekana do bufora.
- Symulacja działa w osobnym wątku z precyzyjnym taktowaniem 60 Hz; WebSocket zawsze wysyła najświeższą klatkę (wolny klient gubi klatki zamiast budować kolejkę).
- Binarny protokół: pola jako `int16`, maska ciał upakowana bitowo; ciśnienie i dym wysyłane tylko gdy są wyświetlane; lista obiektów i mapa etykiet tylko po zmianie geometrii.
- Łagodny rozruch strumienia (1200 kroków) — bez fali uderzeniowej i stojących fal akustycznych.
- Domyślna siatka ~500×280 (do ~790×440 w trybie Maks.) przy 60 fps.
- Frontend: kolory przez LUT, pole renderowane w rozdzielczości siatki i skalowane na GPU, cząstki rysowane jedną ścieżką na klatkę.

## Struktura

```
server/lbm.py   solver LBM (kernel numba), składanie maski z obiektów, cofanie
server/bodies.py obiekty: geometria, przekształcenia, rasteryzacja w bbox z buforowaniem
server/app.py   FastAPI + WebSocket, protokół binarny, scenariusze
server/run.py   start serwera (z rozgrzewką JIT)
web/src/        React: App.tsx (UI, narzędzia), render.ts (mapy kolorów, cząstki), sim.ts (protokół)
```
