use core::ops::{Add, AddAssign, Sub};
use rustfft::{Fft, FftPlanner};
use std::collections::HashMap;
use std::sync::Arc;

/// Kahan compensated summation to reduce rounding-error accumulation.
#[derive(Clone, Copy, Debug)]
pub struct KahanSum<T> {
    sum: T,
    c: T, // compensation
}

impl<T> KahanSum<T>
where
    T: Copy + Default,
{
    #[inline]
    pub(crate) fn new(sum: T) -> Self {
        Self {
            sum,
            c: T::default(),
        }
    }

    #[inline]
    pub(crate) const fn value(self) -> T {
        self.sum
    }
}

impl<T> Default for KahanSum<T>
where
    T: Copy + Default,
{
    #[inline]
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> AddAssign<T> for KahanSum<T>
where
    T: Copy + Sub<Output = T> + Add<Output = T>,
{
    #[inline]
    fn add_assign(&mut self, value: T) {
        let y = value - self.c; // corrected addend
        let t = self.sum + y; // tentative sum
        self.c = (t - self.sum) - y; // new compensation
        self.sum = t;
    }
}

/// First-order sync search result.
#[derive(Clone, Copy, Debug)]
pub struct Sync {
    pub(crate) freq: f32,
    pub(crate) step: f32,
    pub(crate) sync: f32,
}

impl Sync {
    #[inline]
    pub(crate) const fn new(freq: f32, step: f32, sync: f32) -> Self {
        Self { freq, step, sync }
    }
}

/// Decoded message representation: 3-bit type + 12 decoded bytes.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Decode {
    pub(crate) type_id: u8,
    pub(crate) data: [u8; 12],
}

pub type DecodeMap = HashMap<Decode, i32>;

/// `RustFFT` replacement for the FFTW plan manager.
///
/// FFTW plans aren't directly portable since rustfft uses planned FFT objects.
/// This manager provides the same “slot” concept as the C++ plan array,
/// without tying plans to input/output buffers.
pub struct FftPlanManager {
    planner: FftPlanner<f32>,
    plans: [Option<FftPlanSlot>; FftPlanType::COUNT],
}

#[derive(Clone)]
struct FftPlanSlot {
    len: usize,
    inverse: bool,
    fft: Arc<dyn Fft<f32>>,
}

#[derive(Clone, Copy, Debug)]
pub enum FftPlanType {
    DS,
    BB,
    CF,
    CB,
    SD,
    CS,
}

impl FftPlanType {
    pub(crate) const COUNT: usize = 6;

    #[inline]
    const fn idx(self) -> usize {
        match self {
            Self::DS => 0,
            Self::BB => 1,
            Self::CF => 2,
            Self::CB => 3,
            Self::SD => 4,
            Self::CS => 5,
        }
    }
}

impl Default for FftPlanManager {
    fn default() -> Self {
        Self {
            planner: FftPlanner::new(),
            plans: std::array::from_fn(|_| None),
        }
    }
}

impl FftPlanManager {
    #[inline]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Get or create a plan for a slot.
    ///
    /// If the slot already holds a different (len, inverse) plan, it is replaced.
    #[inline]
    pub(crate) fn get_or_create(
        &mut self,
        ty: FftPlanType,
        len: usize,
        inverse: bool,
    ) -> Arc<dyn Fft<f32>> {
        if let Some(p) = self.plans[ty.idx()].as_ref()
            && p.len == len
            && p.inverse == inverse
        {
            return Arc::clone(&p.fft);
        }

        let fft = if inverse {
            self.planner.plan_fft_inverse(len)
        } else {
            self.planner.plan_fft_forward(len)
        };

        self.plans[ty.idx()] = Some(FftPlanSlot {
            len,
            inverse,
            fft: Arc::clone(&fft),
        });
        fft
    }
}

pub mod belief_decoder {

    use crate::internal::consts::{K, M, N};

    use libm::{atanhf, tanhf};

    /// Max rows per column in Nm.
    const BP_MAX_ROWS: usize = 7;
    /// Max checks per bit in Mn.
    const BP_MAX_CHECKS: usize = 3;
    // Max iterations in BP decoder.
    const BP_MAX_ITERATIONS: usize = 30;

    pub const MN: [[i32; BP_MAX_CHECKS]; N] = [
        [0, 24, 68],
        [1, 4, 72],
        [2, 31, 67],
        [3, 50, 60],
        [5, 62, 69],
        [6, 32, 78],
        [7, 49, 85],
        [8, 36, 42],
        [9, 40, 64],
        [10, 13, 63],
        [11, 74, 76],
        [12, 22, 80],
        [14, 15, 81],
        [16, 55, 65],
        [17, 52, 59],
        [18, 30, 51],
        [19, 66, 83],
        [20, 28, 71],
        [21, 23, 43],
        [25, 34, 75],
        [26, 35, 37],
        [27, 39, 41],
        [29, 53, 54],
        [33, 48, 86],
        [38, 56, 57],
        [44, 73, 82],
        [45, 61, 79],
        [46, 47, 84],
        [58, 70, 77],
        [0, 49, 52],
        [1, 46, 83],
        [2, 24, 78],
        [3, 5, 13],
        [4, 6, 79],
        [7, 33, 54],
        [8, 35, 68],
        [9, 42, 82],
        [10, 22, 73],
        [11, 16, 43],
        [12, 56, 75],
        [14, 26, 55],
        [15, 27, 28],
        [17, 18, 58],
        [19, 39, 62],
        [20, 34, 51],
        [21, 53, 63],
        [23, 61, 77],
        [25, 31, 76],
        [29, 71, 84],
        [30, 64, 86],
        [32, 38, 50],
        [36, 47, 74],
        [37, 69, 70],
        [40, 41, 67],
        [44, 66, 85],
        [45, 80, 81],
        [48, 65, 72],
        [57, 59, 65],
        [60, 64, 84],
        [0, 13, 20],
        [1, 12, 58],
        [2, 66, 81],
        [3, 31, 72],
        [4, 35, 53],
        [5, 42, 45],
        [6, 27, 74],
        [7, 32, 70],
        [8, 48, 75],
        [9, 57, 63],
        [10, 47, 67],
        [11, 18, 44],
        [14, 49, 60],
        [15, 21, 25],
        [16, 71, 79],
        [17, 39, 54],
        [19, 34, 50],
        [22, 24, 33],
        [23, 62, 86],
        [26, 38, 73],
        [28, 77, 82],
        [29, 69, 76],
        [30, 68, 83],
        [21, 36, 85],
        [37, 40, 80],
        [41, 43, 56],
        [46, 52, 61],
        [51, 55, 78],
        [59, 74, 80],
        [0, 38, 76],
        [1, 15, 40],
        [2, 30, 53],
        [3, 35, 77],
        [4, 44, 64],
        [5, 56, 84],
        [6, 13, 48],
        [7, 20, 45],
        [8, 14, 71],
        [9, 19, 61],
        [10, 16, 70],
        [11, 33, 46],
        [12, 67, 85],
        [17, 22, 42],
        [18, 63, 72],
        [23, 47, 78],
        [24, 69, 82],
        [25, 79, 86],
        [26, 31, 39],
        [27, 55, 68],
        [28, 62, 65],
        [29, 41, 49],
        [32, 36, 81],
        [34, 59, 73],
        [37, 54, 83],
        [43, 51, 60],
        [50, 52, 71],
        [57, 58, 66],
        [46, 55, 75],
        [0, 18, 36],
        [1, 60, 74],
        [2, 7, 65],
        [3, 59, 83],
        [4, 33, 38],
        [5, 25, 52],
        [6, 31, 56],
        [8, 51, 66],
        [9, 11, 14],
        [10, 50, 68],
        [12, 13, 64],
        [15, 30, 42],
        [16, 19, 35],
        [17, 79, 85],
        [20, 47, 58],
        [21, 39, 45],
        [22, 32, 61],
        [23, 29, 73],
        [24, 41, 63],
        [26, 48, 84],
        [27, 37, 72],
        [28, 43, 80],
        [34, 67, 69],
        [40, 62, 75],
        [44, 48, 70],
        [49, 57, 86],
        [47, 53, 82],
        [12, 54, 78],
        [76, 77, 81],
        [0, 1, 23],
        [2, 5, 74],
        [3, 55, 86],
        [4, 43, 52],
        [6, 49, 82],
        [7, 9, 27],
        [8, 54, 61],
        [10, 28, 66],
        [11, 32, 39],
        [13, 15, 19],
        [14, 34, 72],
        [16, 30, 38],
        [17, 35, 56],
        [18, 45, 75],
        [20, 41, 83],
        [21, 33, 58],
        [22, 25, 60],
        [24, 59, 64],
        [26, 63, 79],
        [29, 36, 65],
        [31, 44, 71],
        [37, 50, 85],
        [40, 76, 78],
        [42, 55, 67],
        [46, 73, 81],
        [39, 51, 77],
        [53, 60, 70],
        [45, 57, 68],
    ];

    #[derive(Clone, Copy, Debug)]
    pub struct CheckNode {
        pub(crate) valid_neighbors: usize,
        pub(crate) neighbors: [usize; BP_MAX_ROWS],
    }

    pub const NM: [CheckNode; M] = [
        CheckNode {
            valid_neighbors: 6,
            neighbors: [0, 29, 59, 88, 117, 146, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [1, 30, 60, 89, 118, 146, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [2, 31, 61, 90, 119, 147, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [3, 32, 62, 91, 120, 148, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [1, 33, 63, 92, 121, 149, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [4, 32, 64, 93, 122, 147, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [5, 33, 65, 94, 123, 150, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [6, 34, 66, 95, 119, 151, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [7, 35, 67, 96, 124, 152, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [8, 36, 68, 97, 125, 151, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [9, 37, 69, 98, 126, 153, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [10, 38, 70, 99, 125, 154, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [11, 39, 60, 100, 127, 144, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [9, 32, 59, 94, 127, 155, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [12, 40, 71, 96, 125, 156, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [12, 41, 72, 89, 128, 155, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [13, 38, 73, 98, 129, 157, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [14, 42, 74, 101, 130, 158, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [15, 42, 70, 102, 117, 159, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [16, 43, 75, 97, 129, 155, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [17, 44, 59, 95, 131, 160, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [18, 45, 72, 82, 132, 161, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [11, 37, 76, 101, 133, 162, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [18, 46, 77, 103, 134, 146, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [0, 31, 76, 104, 135, 163, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [19, 47, 72, 105, 122, 162, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [20, 40, 78, 106, 136, 164, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [21, 41, 65, 107, 137, 151, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [17, 41, 79, 108, 138, 153, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [22, 48, 80, 109, 134, 165, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [15, 49, 81, 90, 128, 157, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [2, 47, 62, 106, 123, 166, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [5, 50, 66, 110, 133, 154, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [23, 34, 76, 99, 121, 161, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [19, 44, 75, 111, 139, 156, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [20, 35, 63, 91, 129, 158, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [7, 51, 82, 110, 117, 165, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [20, 52, 83, 112, 137, 167, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [24, 50, 78, 88, 121, 157, 0],
        },
        CheckNode {
            valid_neighbors: 7,
            neighbors: [21, 43, 74, 106, 132, 154, 171],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [8, 53, 83, 89, 140, 168, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [21, 53, 84, 109, 135, 160, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [7, 36, 64, 101, 128, 169, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [18, 38, 84, 113, 138, 149, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [25, 54, 70, 92, 141, 166, 0],
        },
        CheckNode {
            valid_neighbors: 7,
            neighbors: [26, 55, 64, 95, 132, 159, 173],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [27, 30, 85, 99, 116, 170, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [27, 51, 69, 103, 131, 143, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [23, 56, 67, 94, 136, 141, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [6, 29, 71, 109, 142, 150, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [3, 50, 75, 114, 126, 167, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [15, 44, 86, 113, 124, 171, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [14, 29, 85, 114, 122, 149, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [22, 45, 63, 90, 143, 172, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [22, 34, 74, 112, 144, 152, 0],
        },
        CheckNode {
            valid_neighbors: 7,
            neighbors: [13, 40, 86, 107, 116, 148, 169],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [24, 39, 84, 93, 123, 158, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [24, 57, 68, 115, 142, 173, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [28, 42, 60, 115, 131, 161, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [14, 57, 87, 111, 120, 163, 0],
        },
        CheckNode {
            valid_neighbors: 7,
            neighbors: [3, 58, 71, 113, 118, 162, 172],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [26, 46, 85, 97, 133, 152, 0],
        },
        CheckNode {
            valid_neighbors: 5,
            neighbors: [4, 43, 77, 108, 140, 0, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [9, 45, 68, 102, 135, 164, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [8, 49, 58, 92, 127, 163, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [13, 56, 57, 108, 119, 165, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [16, 54, 61, 115, 124, 153, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [2, 53, 69, 100, 139, 169, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [0, 35, 81, 107, 126, 173, 0],
        },
        CheckNode {
            valid_neighbors: 5,
            neighbors: [4, 52, 80, 104, 139, 0, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [28, 52, 66, 98, 141, 172, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [17, 48, 73, 96, 114, 166, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [1, 56, 62, 102, 137, 156, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [25, 37, 78, 111, 134, 170, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [10, 51, 65, 87, 118, 147, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [19, 39, 67, 116, 140, 159, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [10, 47, 80, 88, 145, 168, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [28, 46, 79, 91, 145, 171, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [5, 31, 86, 103, 144, 168, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [26, 33, 73, 105, 130, 164, 0],
        },
        CheckNode {
            valid_neighbors: 5,
            neighbors: [11, 55, 83, 87, 138, 0, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [12, 55, 61, 110, 145, 170, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [25, 36, 79, 104, 143, 150, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [16, 30, 81, 112, 120, 160, 0],
        },
        CheckNode {
            valid_neighbors: 5,
            neighbors: [27, 48, 58, 93, 136, 0, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [6, 54, 82, 100, 130, 167, 0],
        },
        CheckNode {
            valid_neighbors: 6,
            neighbors: [23, 49, 77, 105, 142, 148, 0],
        },
    ];

    /// Belief-propagation decode for N=174, K=87, M=87.
    ///
    /// Returns:
    /// - `>= 0`: number of bit decisions inconsistent with channel LLR signs
    /// - `-1`: decoding failed / early-stopped
    pub fn bpdecode174(llr: &[f32; N], decoded: &mut [i8; K], cw: &mut [i8; N]) -> i32 {
        let mut tov = [[0.0f32; BP_MAX_CHECKS]; N];
        let mut toc = [[0.0f32; BP_MAX_ROWS]; M];
        let mut tanhtoc = [[0.0f32; BP_MAX_ROWS]; M];
        let mut zn = [0.0f32; N];
        let mut synd = [0i32; M];
        let mut ncnt: i32 = 0;
        let mut nclast: i32 = 0;
        for i in 0..M {
            let vn = NM[i].valid_neighbors;
            for (j, bit) in NM[i].neighbors.iter().copied().take(vn).enumerate() {
                toc[i][j] = llr[bit];
            }
        }
        for iter in 0..=BP_MAX_ITERATIONS {
            for i in 0..N {
                zn[i] = llr[i] + tov[i][0] + tov[i][1] + tov[i][2];
            }

            for i in 0..N {
                cw[i] = i8::from(zn[i] > 0.0);
            }

            let mut ncheck: i32 = 0;
            for i in 0..M {
                let mut sum: i32 = 0;
                let vn = NM[i].valid_neighbors;
                for j in 0..vn {
                    sum += i32::from(cw[NM[i].neighbors[j]]);
                }
                synd[i] = sum;
                if (sum & 1) != 0 {
                    ncheck += 1;
                }
            }

            if ncheck == 0 {
                decoded[..K].copy_from_slice(&cw[M..(K + M)]);

                let mut nerr: i32 = 0;
                for i in 0..N {
                    let hard = (2 * i32::from(cw[i]) - 1) as f32;
                    if hard * llr[i] < 0.0 {
                        nerr += 1;
                    }
                }
                return nerr;
            }

            if iter > 0 {
                let nd = ncheck - nclast;
                ncnt = if nd < 0 { 0 } else { ncnt + 1 };
                if ncnt >= 5 && (iter as i32) >= 10 && ncheck > 15 {
                    return -1;
                }
            }
            nclast = ncheck;

            for i in 0..M {
                let vn = NM[i].valid_neighbors;
                for (j, bit) in NM[i].neighbors.iter().copied().take(vn).enumerate() {
                    let mut msg = zn[bit];
                    for k in 0..BP_MAX_CHECKS {
                        if MN[bit][k] == i as i32 {
                            msg -= tov[bit][k];
                        }
                    }

                    toc[i][j] = msg;
                }
            }
            for i in 0..M {
                for j in 0..BP_MAX_ROWS {
                    tanhtoc[i][j] = tanhf(-toc[i][j] * 0.5);
                }
            }

            for i in 0..N {
                for j in 0..BP_MAX_CHECKS {
                    let ichk_i32 = MN[i][j];
                    if ichk_i32 >= 0 {
                        let ichk = ichk_i32 as usize;

                        let mut tmn: f32 = 1.0;
                        let vn = NM[ichk].valid_neighbors;
                        for (k, neighbor) in NM[ichk].neighbors.iter().take(vn).enumerate() {
                            if *neighbor != i {
                                tmn *= tanhtoc[ichk][k];
                            }
                        }

                        tov[i][j] = 2.0 * atanhf(-tmn);
                    }
                }
            }
        }

        -1
    }
}
