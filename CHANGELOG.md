# Historia zmian

Format: [Keep a Changelog](https://keepachangelog.com/pl/1.1.0/), wersjonowanie: [SemVer](https://semver.org/lang/pl/).

## [1.1.1] — 2026-10-15

### Zmienione
- Karta graficzna jest używana zawsze, gdy jest dostępna — bez przełącznika w interfejsie. Bez karty albo po błędzie sterownika program po cichu i bez zatrzymywania symulacji liczy na procesorze.
- Nowe domyślne tempo **Płynne**: strumień przechodzi przez widok w ok. 8 s niezależnie od sprzętu. Na GPU obraz nie pędzi już kilkanaście razy szybciej niż wcześniej. Dotychczasowe tempo bez ograniczeń jest dostępne jako **Maksymalne**.

### Poprawione
- Cząstki i dym są przesuwane podkrokami, więc nadążają za przepływem także przy setkach kroków solvera na klatkę. Wcześniej cząstki przesuwały się najwyżej o 64 kroki na klatkę i przy szybkim GPU wiry wyprzedzały cząstki.

## [1.1.0] — 2026-10-15

### Obliczenia na karcie graficznej
- Solver działa na **GPU** (wgpu: Vulkan, DirectX 12, Metal) — kilkanaście razy szybciej niż na procesorze (np. RTX 4070 Ti: ok. 4800 MLUPS na siatce 1,4 mln komórek wobec ok. 300 MLUPS na CPU), więc w czasie rzeczywistym da się liczyć znacznie gęstsze siatki, a przez to wyższe liczby Reynoldsa.
- Jądro GPU jest wiernym odpowiednikiem jądra CPU (te same brzegi, odbicie Bouzidiego, LES, siły); test automatyczny porównuje pola i siły obu wersji.
- Gdy karty nie ma, sterownik jej nie udostępnia albo przestanie odpowiadać, program sam liczy na procesorze. Plakietka **GPU / CPU** w górnym pasku pokazuje, co liczy, i przełącza jednym kliknięciem.

### Nowy interfejs
- Górny pasek prowadzi przez pracę w kolejności: **scenariusz** (📂 z nazwą sceny) → **Start / Pauza** → **co pokazać** (Prędkość, Ciśnienie, Wirowość, Dym, Więcej) → nakładki (cząstki, linie prądu, wektory, siły). Pozostałe polecenia są w jednym menu ☰.
- Pasek narzędzi z **podpisami** i grupami (Ciała, Ściany, Pomiar); podpowiedź aktywnego narzędzia na dole widoku.
- Panel **Inspektor** zamiast czterech zakładek: na górze karta **Wyniki** (Cd, Cl, L/D lub St, Re, siły w N, stan ustalenia i wiarygodność), pod nią zaznaczony obiekt albo lista obiektów, potem **Warunki przepływu**. Obszar i brzegi, wyświetlanie oraz obliczenia są w zwijanych sekcjach.
- Pasek stanu: czy liczy, czas fizyczny, tempo względem rzeczywistości, urządzenie i wydajność.

### Ciśnienie
- Skala kolorów ciśnienia i Cp ma teraz stałe znaczenie fizyczne: **biel = ciśnienie otoczenia**, pełna czerwień = ciśnienie spiętrzenia (Cp = 1), błękit = ssanie skalowane do najsilniejszego podciśnienia. Wcześniej skala dobierała się sama do 98,5. percentyla |Cp|, przez co zwykłe podciśnienie przy grubym profilu (Cp ≈ −0,3) wyglądało jak silne ssanie i obraz wokół skrzydła wydawał się niefizyczny.
- Test walidacyjny rozkładu ciśnienia na NACA 0012 (Cp z obu stron przy α = 0°, różnica ciśnień przy α = 4°).

### Wydania
- Wersja przenośna to teraz **jeden plik `.exe`** bez archiwum.

## [1.0.0] — 2026-10-14

Pierwsze wydanie przepisane od zera w Rust jako natywna aplikacja z instalatorem.

### Fizyka
- Obliczenia w **jednostkach fizycznych**: wybór płynu (powietrze — gaz doskonały i wzór Sutherlanda, woda — równania Thiesena i Vogela, płyn własny), temperatury, prędkości i rozmiaru obszaru. Liczba Reynoldsa, Macha, siły w niutonach i ciśnienie w paskalach wynikają z rzeczywistych własności płynu.
- Solver Lattice-Boltzmanna D2Q9 z regularyzowaną kolizją i modelem LES Smagorinsky'ego (poprawny współczynnik 18√2), bez sztucznego ograniczania prędkości i gęstości; układ pamięci SoA z szybką ścieżką dla komórek wewnętrznych.
- Osobna lepkość objętościowa tłumi fale akustyczne modelu słabo ściśliwego, a minimalna lepkość numeryczna (τ ≥ 0,503) usuwa prążki siatkowe przy bardzo dużych Re — program pokazuje wtedy efektywną liczbę Reynoldsa siatki.
- Wlot daleko przed widocznym obszarem; ciała dotykające krawędzi obszaru przedłużają się w niewidoczne marginesy.
- Siły liczone od nadciśnienia względem strumienia swobodnego — poprawne także dla ciał stojących na podłodze (budynki, samochód).
- **Interpolowane odbicie Bouzidiego** z dokładną odległością ściany — krzywe brzegi z drugim rzędem dokładności zamiast „schodków”.
- Siły metodą wymiany pędu dla **każdego ciała osobno**, łącznie z momentem pochylającym (Cm).
- Warunki brzegowe: opływ swobodny, ściana tunelu, ściana bez tarcia, **ruchoma podłoga**; profile wlotu: jednorodny, kanałowy, **warstwa przyziemna**; turbulencja wlotu o zadanej intensywności.
- **Ocena wiarygodności** obliczeń (rozdzielczość warstwy przyściennej, τ, Ma, rozmiar ciała) z wyjaśnieniem ograniczeń modelu 2D.
- Scenariusze walidacyjne z wartościami z literatury (walec Re 20/100/200, pręt kwadratowy) i porównaniem wyników.

### Analiza
- Historia Cd, Cl, Cm w czasie, średnie po pełnych okresach, amplituda, **liczba Strouhala** i częstotliwość odrywania wirów.
- Rozkład **Cp na powierzchni** (x/c dla profili, kąt dla walców).
- **Automatyczna biegunowa** profilu: Cl(α), Cd(α), Cm(α) i Cl(Cd).
- **Sondy** punktowe z przebiegiem prędkości i ciśnienia oraz widmem częstotliwości.
- Uśrednianie w czasie: średnie pole prędkości i ciśnienia, intensywność turbulencji.
- Eksport CSV (siły, biegunowa, sondy) i zrzut ekranu PNG.

### Wizualizacja
- Pola: prędkość, składowe, wirowość, ciśnienie (Pa) i Cp, kryterium Q, dym, pola średnie, intensywność turbulencji, lepkość turbulentna.
- Cząstki ze śladami, linie prądu o równym odstępie, wektory prędkości, wektory sił na ciałach, legenda z jednostkami.
- Powiększanie i przesuwanie widoku.

### Edycja
- Profile NACA (dowolne 4-cyfrowe), walce, elipsy, prostokąty z zaokrągleniem, dowolne kształty, ściany malowane pędzlem z gumką, źródła dymu, sondy.
- Przesuwanie, obrót uchwytem, kółkiem i klawiszami, menu kontekstowe, lista obiektów z edycją parametrów, cofanie/ponawianie.
- Zapis i odczyt scen (`.aeroflow`), ostatnia scena przywracana przy starcie.

### Aplikacja
- Instalator Windows z menu Start, skrótem i skojarzeniem plików `.aeroflow`.
- **Powiadomienia o aktualizacjach**, pobieranie z weryfikacją SHA-256 i instalacja jednym kliknięciem; okno „Co nowego” po aktualizacji.
- Automatyczne wydania przez GitHub Actions.
