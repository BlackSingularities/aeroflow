# Historia zmian

Format: [Keep a Changelog](https://keepachangelog.com/pl/1.1.0/), wersjonowanie: [SemVer](https://semver.org/lang/pl/).

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
