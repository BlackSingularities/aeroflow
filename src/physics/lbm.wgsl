// Jeden krok solvera D2Q9 na GPU — wierny odpowiednik `Lattice::step_once` (lattice.rs):
// strumieniowanie „pull”, odbicie Bouzidiego na ciałach, brzegi, regularyzowana kolizja BGK z modelem
// Smagorinsky'ego, strefy pochłaniające i wymiana pędu dla sił. Jeden wątek = jedna komórka.

struct Params {
    nx: u32,
    ny: u32,
    n: u32,
    nbodies: u32,
    tau0: f32,
    les_k: f32,
    u_wall: f32,
    keep_bulk: f32,
    top: u32,
    bottom: u32,
    groups_x: u32,
    turb_on: u32,
    turbulence: f32,
    u_lb: f32,
    win_h: f32,
    _pad0: f32,
    o_sponge_row: u32,
    o_sponge_col: u32,
    o_eq: u32,
    o_profile: u32,
    o_pivot: u32,
    o_phase: u32,
    o_linkq: u32,
    _pad1: u32,
}

struct Step {
    step: u32,
    rho_ref: f32,
    _a: u32,
    _b: u32,
}

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<uniform> S: Step;
@group(0) @binding(2) var<storage, read> fsrc: array<f32>;
@group(0) @binding(3) var<storage, read_write> fdst: array<f32>;
// wielkości makroskopowe [ρ | ux | uy | ν_t], każda po n wartości
@group(0) @binding(4) var<storage, read> msrc: array<f32>;
@group(0) @binding(5) var<storage, read_write> mdst: array<f32>;
// [rodzaj | etykieta << 8] dla n komórek, potem indeks połączenia dla n komórek
@group(0) @binding(6) var<storage, read> cell: array<u32>;
@group(0) @binding(7) var<storage, read> aux: array<f32>;
// wymiana pędu dla każdego (połączenie, kierunek): [Fx, Fy, Mz], sumowana przez kolejne kroki
@group(0) @binding(8) var<storage, read_write> force: array<f32>;
@group(0) @binding(9) var<storage, read_write> flags: array<atomic<u32>>;

const K_FLUID: u32 = 0u;
const K_SOLID: u32 = 1u;
const K_INLET: u32 = 2u;
const NO_LINK: u32 = 0xffffffffu;
const B_FREE: u32 = 0u;
const B_SLIP: u32 = 1u;
const B_NOSLIP: u32 = 2u;
const TAU: f32 = 6.283185307179586;

var<private> CX: array<i32, 9> = array<i32, 9>(0, 1, 0, -1, 0, 1, -1, -1, 1);
var<private> CY: array<i32, 9> = array<i32, 9>(0, 0, 1, 0, -1, 1, 1, -1, -1);
var<private> W: array<f32, 9> = array<f32, 9>(
    0.44444445, 0.11111111, 0.11111111, 0.11111111, 0.11111111,
    0.027777778, 0.027777778, 0.027777778, 0.027777778
);
var<private> OPP: array<u32, 9> = array<u32, 9>(0u, 3u, 4u, 1u, 2u, 7u, 8u, 5u, 6u);
var<private> MIRY: array<u32, 9> = array<u32, 9>(0u, 1u, 4u, 3u, 2u, 8u, 7u, 6u, 5u);

fn feq(i: u32, r: f32, u: f32, v: f32) -> f32 {
    let cu = 3.0 * (f32(CX[i]) * u + f32(CY[i]) * v);
    return W[i] * r * (1.0 + cu + 0.5 * cu * cu - 1.5 * (u * u + v * v));
}

fn label(c: u32) -> u32 {
    return cell[c] >> 8u;
}

// profil wlotu z syntetyczną turbulencją (suma modów o losowych fazach)
fn inlet(y: u32) -> vec2<f32> {
    var u = aux[P.o_profile + y];
    var v = 0.0;
    if (P.turb_on != 0u) {
        let t = f32(S.step) * P.u_lb / P.win_h;
        let norm = P.turbulence * P.u_lb / 0.51;
        let s = f32(y) / f32(P.ny);
        var du = 0.0;
        var dv = 0.0;
        for (var k = 0u; k < 8u; k++) {
            let kk = f32(k + 2u);
            du += sin(TAU * (kk * s + 1.7 * kk * t) + aux[P.o_phase + 2u * k]) / kk;
            dv += sin(TAU * (kk * s - 1.3 * kk * t) + aux[P.o_phase + 2u * k + 1u]) / kk;
        }
        u += du * norm;
        v += dv * norm;
    }
    return vec2<f32>(u, v);
}

@compute @workgroup_size(128)
fn step(@builtin(global_invocation_id) gid: vec3<u32>) {
    let c = gid.x + gid.y * P.groups_x * 128u;
    let n = P.n;
    if (c >= n) {
        return;
    }
    let nx = P.nx;
    let ny = P.ny;
    let x = c % nx;
    let y = c / nx;
    let kind = cell[c] & 3u;

    if (kind == K_SOLID) {
        for (var i = 0u; i < 9u; i++) {
            fdst[i * n + c] = W[i];
        }
        mdst[c] = 1.0;
        mdst[n + c] = 0.0;
        mdst[2u * n + c] = 0.0;
        mdst[3u * n + c] = 0.0;
        return;
    }
    if (kind == K_INLET) {
        let r = msrc[y * nx + 1u];
        let iuv = inlet(y);
        for (var i = 0u; i < 9u; i++) {
            fdst[i * n + c] = feq(i, r, iuv.x, iuv.y);
        }
        mdst[c] = r;
        mdst[n + c] = iuv.x;
        mdst[2u * n + c] = iuv.y;
        mdst[3u * n + c] = 0.0;
        return;
    }

    var fl: array<f32, 9>;
    if (kind == K_FLUID) {
        for (var i = 0u; i < 9u; i++) {
            let src = u32(i32(c) - CX[i] - CY[i] * i32(nx));
            fl[i] = fsrc[i * n + src];
        }
    } else {
        let li = cell[n + c];
        for (var i = 0u; i < 9u; i++) {
            let sx = i32(x) - CX[i];
            let sy = i32(y) - CY[i];
            if (sy < 0 || sy >= i32(ny)) {
                let side = select(P.top, P.bottom, sy < 0);
                if (side == B_FREE) {
                    fl[i] = aux[P.o_eq + y * 9u + i];
                } else if (side == B_SLIP) {
                    let sc = y * nx + u32(clamp(sx, 0, i32(nx) - 1));
                    if (label(sc) == 0u) {
                        fl[i] = fsrc[MIRY[i] * n + sc];
                    } else {
                        fl[i] = fsrc[MIRY[i] * n + c];
                    }
                } else if (side == B_NOSLIP) {
                    fl[i] = fsrc[OPP[i] * n + c];
                } else {
                    fl[i] = fsrc[OPP[i] * n + c] + 6.0 * W[i] * f32(CX[i]) * P.u_wall;
                }
                continue;
            }
            if (sx >= i32(nx)) {
                // wylot: zerowy gradient
                fl[i] = fsrc[i * n + c - 1u];
                continue;
            }
            let sc = u32(sy) * nx + u32(sx);
            let lab = label(sc);
            if (lab == 0u) {
                fl[i] = fsrc[i * n + sc];
                continue;
            }
            // ściana w kierunku k = −c_i od tej komórki (Bouzidi)
            let k = OPP[i];
            var q = 0.5;
            if (li != NO_LINK) {
                q = aux[P.o_linkq + li * 9u + k];
            }
            if (q <= 0.0) {
                q = 0.5;
            }
            let fk = fsrc[k * n + c];
            var val = fk;
            if (q < 0.5) {
                let x2 = i32(x) + CX[i];
                let y2 = i32(y) + CY[i];
                if (x2 >= 0 && y2 >= 0 && x2 < i32(nx) && y2 < i32(ny)) {
                    let c2 = u32(y2) * nx + u32(x2);
                    if (label(c2) == 0u) {
                        val = 2.0 * q * fk + (1.0 - 2.0 * q) * fsrc[k * n + c2];
                    }
                }
            } else {
                val = fk / (2.0 * q) + (2.0 * q - 1.0) / (2.0 * q) * fsrc[i * n + c];
            }
            fl[i] = val;
            // pęd przekazany ciału
            let b = lab - 1u;
            if (b < P.nbodies && li != NO_LINK) {
                let s = fk + val - 2.0 * W[k] * S.rho_ref;
                let fx = f32(CX[k]) * s;
                let fy = f32(CY[k]) * s;
                let wx = f32(x) + 0.5 + q * f32(CX[k]) - aux[P.o_pivot + 2u * b];
                let wy = f32(y) + 0.5 + q * f32(CY[k]) - aux[P.o_pivot + 2u * b + 1u];
                let base = (li * 9u + i) * 3u;
                force[base] += fx;
                force[base + 1u] += fy;
                force[base + 2u] += wx * fy - wy * fx;
            }
        }
    }

    // momenty: gęstość, pęd i tensor naprężeń
    var r = fl[0] + fl[1] + fl[2] + fl[3] + fl[4] + fl[5] + fl[6] + fl[7] + fl[8];
    let jx = fl[1] - fl[3] + fl[5] - fl[6] - fl[7] + fl[8];
    let jy = fl[2] - fl[4] + fl[5] + fl[6] - fl[7] - fl[8];
    let diag = fl[5] + fl[6] + fl[7] + fl[8];
    var pxx = fl[1] + fl[3] + diag;
    var pyy = fl[2] + fl[4] + diag;
    var pxy = fl[5] - fl[6] + fl[7] - fl[8];
    if (!(r > 0.2 && r < 5.0)) {
        atomicStore(&flags[0], 1u);
        r = clamp(r, 0.2, 5.0);
    }
    let inv = 1.0 / r;
    let u = jx * inv;
    let v = jy * inv;
    pxx -= r * (1.0 / 3.0 + u * u);
    pyy -= r * (1.0 / 3.0 + v * v);
    pxy -= r * u * v;
    let qn = sqrt(pxx * pxx + pyy * pyy + 2.0 * pxy * pxy);
    let tau = 0.5 * (P.tau0 + sqrt(P.tau0 * P.tau0 + P.les_k * qn * inv));
    let keep = 1.0 - 1.0 / tau;
    let sg = max(aux[P.o_sponge_row + y], aux[P.o_sponge_col + x]);
    let usq = 1.5 * (u * u + v * v);
    let tr = 0.5 * (pxx + pyy);
    let axx = 4.5 * (keep * (pxx - tr) + P.keep_bulk * tr);
    let ayy = 4.5 * (keep * (pyy - tr) + P.keep_bulk * tr);
    let axy = 9.0 * keep * pxy;
    let t0 = -(axx + ayy) / 3.0;
    for (var i = 0u; i < 9u; i++) {
        let cx = f32(CX[i]);
        let cy = f32(CY[i]);
        let cu = 3.0 * (cx * u + cy * v);
        let neq = t0 + cx * cx * axx + cy * cy * ayy + cx * cy * axy;
        var post = W[i] * (r * (1.0 + cu + 0.5 * cu * cu - usq) + neq);
        if (sg > 0.0) {
            post += sg * (aux[P.o_eq + y * 9u + i] - post);
        }
        fdst[i * n + c] = post;
    }
    mdst[c] = r;
    mdst[n + c] = u;
    mdst[2u * n + c] = v;
    mdst[3u * n + c] = (tau - P.tau0) / 3.0;
}
