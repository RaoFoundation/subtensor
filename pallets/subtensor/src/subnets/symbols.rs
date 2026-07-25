//! Default subnet token symbols and human-readable name tables.
//!
//! [`SYMBOLS`] is indexed by netuid for the default glyph; [`get_name_for_subnet`]
//! maps netuid to a UTF-8 name used in RPC / UI surfaces.

use super::*;
use sp_std::collections::btree_set::BTreeSet;
use subtensor_runtime_common::NetUid;

// TODO: default symbol should be different from the root symbol?
pub static DEFAULT_SYMBOL: &[u8] = b"\xCE\xA4"; // TAO uppercase symbol

pub static SYMBOLS: [&[u8]; 439] = [
    // Greek Alphabet (Lowercase)
    DEFAULT_SYMBOL, // Τ (Upper case Tau, 0)
    b"\xCE\xB1",    // α (Alpha, 1)
    b"\xCE\xB2",    // β (Beta, 2)
    b"\xCE\xB3",    // γ (Gamma, 3)
    b"\xCE\xB4",    // δ (Delta, 4)
    b"\xCE\xB5",    // ε (Epsilon, 5)
    b"\xCE\xB6",    // ζ (Zeta, 6)
    b"\xCE\xB7",    // η (Eta, 7)
    b"\xCE\xB8",    // θ (Theta, 8)
    b"\xCE\xB9",    // ι (Iota, 9)
    b"\xCE\xBA",    // κ (Kappa, 10)
    b"\xCE\xBB",    // λ (Lambda, 11)
    b"\xCE\xBC",    // μ (Mu, 12)
    b"\xCE\xBD",    // ν (Nu, 13)
    b"\xCE\xBE",    // ξ (Xi, 14)
    b"\xCE\xBF",    // ο (Omicron, 15)
    b"\xCF\x80",    // π (Pi, 16)
    b"\xCF\x81",    // ρ (Rho, 17)
    b"\xCF\x83",    // σ (Sigma, 18)
    b"t",           // t (Tau, 19)
    b"\xCF\x85",    // υ (Upsilon, 20)
    b"\xCF\x86",    // φ (Phi, 21)
    b"\xCF\x87",    // χ (Chi, 22)
    b"\xCF\x88",    // ψ (Psi, 23)
    b"\xCF\x89",    // ω (Omega, 24)
    // Hebrew Alphabet (Including Final Forms)
    b"\xD7\x90", // א (Aleph, 25)
    b"\xD7\x91", // ב (Bet, 26)
    b"\xD7\x92", // ג (Gimel, 27)
    b"\xD7\x93", // ד (Dalet, 28)
    b"\xD7\x94", // ה (He, 29)
    b"\xD7\x95", // ו (Vav, 30)
    b"\xD7\x96", // ז (Zayin, 31)
    b"\xD7\x97", // ח (Het, 32)
    b"\xD7\x98", // ט (Tet, 33)
    b"\xD7\x99", // י (Yod, 34)
    b"\xD7\x9A", // ך (Final Kaf, 35)
    b"\xD7\x9B", // כ (Kaf, 36)
    b"\xD7\x9C", // ל (Lamed, 37)
    b"\xD7\x9D", // ם (Final Mem, 38)
    b"\xD7\x9E", // מ (Mem, 39)
    b"\xD7\x9F", // ן (Final Nun, 40)
    b"\xD7\xA0", // נ (Nun, 41)
    b"\xD7\xA1", // ס (Samekh, 42)
    b"\xD7\xA2", // ע (Ayin, 43)
    b"\xD7\xA3", // ף (Final Pe, 44)
    b"\xD7\xA4", // פ (Pe, 45)
    b"\xD7\xA5", // ץ (Final Tsadi, 46)
    b"\xD7\xA6", // צ (Tsadi, 47)
    b"\xD7\xA7", // ק (Qof, 48)
    b"\xD7\xA8", // ר (Resh, 49)
    b"\xD7\xA9", // ש (Shin, 50)
    b"\xD7\xAA", // ת (Tav, 51)
    // Arabic Alphabet
    b"\xD8\xA7", // ا (Alif, 52)
    b"\xD8\xA8", // ب (Ba, 53)
    b"\xD8\xAA", // ت (Ta, 54)
    b"\xD8\xAB", // ث (Tha, 55)
    b"\xD8\xAC", // ج (Jim, 56)
    b"\xD8\xAD", // ح (Ha, 57)
    b"\xD8\xAE", // خ (Kha, 58)
    b"\xD8\xAF", // د (Dal, 59)
    b"\xD8\xB0", // ذ (Dhal, 60)
    b"\xD8\xB1", // ر (Ra, 61)
    b"\xD8\xB2", // ز (Zay, 62)
    b"\xD8\xB3", // س (Sin, 63)
    b"\xD8\xB4", // ش (Shin, 64)
    b"\xD8\xB5", // ص (Sad, 65)
    b"\xD8\xB6", // ض (Dad, 66)
    b"\xD8\xB7", // ط (Ta, 67)
    b"\xD8\xB8", // ظ (Dha, 68)
    b"\xD8\xB9", // ع (Ain, 69)
    b"\xD8\xBA", // غ (Ghayn, 70)
    b"\xD9\x81", // ف (Fa, 71)
    b"\xD9\x82", // ق (Qaf, 72)
    b"\xD9\x83", // ك (Kaf, 73)
    b"\xD9\x84", // ل (Lam, 74)
    b"\xD9\x85", // م (Mim, 75)
    b"\xD9\x86", // ن (Nun, 76)
    b"\xD9\x87", // ه (Ha, 77)
    b"\xD9\x88", // و (Waw, 78)
    b"\xD9\x8A", // ي (Ya, 79)
    b"\xD9\x89", // ى (Alef Maksura, 80)
    // Runic Alphabet
    b"\xE1\x9A\xA0", // ᚠ (Fehu, wealth, 81)
    b"\xE1\x9A\xA2", // ᚢ (Uruz, strength, 82)
    b"\xE1\x9A\xA6", // ᚦ (Thurisaz, giant, 83)
    b"\xE1\x9A\xA8", // ᚨ (Ansuz, god, 84)
    b"\xE1\x9A\xB1", // ᚱ (Raidho, ride, 85)
    b"\xE1\x9A\xB3", // ᚳ (Kaunan, ulcer, 86)
    b"\xD0\xAB",     // Ы (Cyrillic Yeru, 87)
    b"\xE1\x9B\x89", // ᛉ (Algiz, protection, 88)
    b"\xE1\x9B\x92", // ᛒ (Berkanan, birch, 89)
    // Ogham Alphabet
    b"\xE1\x9A\x80", //   (Space, 90)
    b"\xE1\x9A\x81", // ᚁ (Beith, birch, 91)
    b"\xE1\x9A\x82", // ᚂ (Luis, rowan, 92)
    b"\xE1\x9A\x83", // ᚃ (Fearn, alder, 93)
    b"\xE1\x9A\x84", // ᚄ (Sail, willow, 94)
    b"\xE1\x9A\x85", // ᚅ (Nion, ash, 95)
    b"\xE1\x9A\x9B", // ᚛ (Forfeda, 96)
    // Georgian Alphabet (Mkhedruli)
    b"\xE1\x83\x90", // ა (Ani, 97)
    b"\xE1\x83\x91", // ბ (Bani, 98)
    b"\xE1\x83\x92", // გ (Gani, 99)
    b"\xE1\x83\x93", // დ (Doni, 100)
    b"\xE1\x83\x94", // ე (Eni, 101)
    b"\xE1\x83\x95", // ვ (Vini, 102)
    // Armenian Alphabet
    b"\xD4\xB1", // Ա (Ayp, 103)
    b"\xD4\xB2", // Բ (Ben, 104)
    b"\xD4\xB3", // Գ (Gim, 105)
    b"\xD4\xB4", // Դ (Da, 106)
    b"\xD4\xB5", // Ե (Ech, 107)
    b"\xD4\xB6", // Զ (Za, 108)
    b"\xD5\x9E", // ՞ (Question mark, 109)
    // Cyrillic Alphabet
    b"\xD0\x80", // Ѐ (Ie with grave, 110)
    b"\xD0\x81", // Ё (Io, 111)
    b"\xD0\x82", // Ђ (Dje, 112)
    b"\xD0\x83", // Ѓ (Gje, 113)
    b"\xD0\x84", // Є (Ukrainian Ie, 114)
    b"\xD0\x85", // Ѕ (Dze, 115)
    b"\xD1\x8A", // ъ (Hard sign, 116)
    // Coptic Alphabet
    b"\xE2\xB2\x80", // Ⲁ (Alfa, 117)
    b"\xE2\xB2\x81", // ⲁ (Small Alfa, 118)
    b"\xE2\xB2\x82", // Ⲃ (Vida, 119)
    b"\xE2\xB2\x83", // ⲃ (Small Vida, 120)
    b"\xE2\xB2\x84", // Ⲅ (Gamma, 121)
    b"\xE2\xB2\x85", // ⲅ (Small Gamma, 122)
    // Brahmi Script
    b"\xF0\x91\x80\x80", // 𑀀 (A, 123)
    b"\xF0\x91\x80\x81", // 𑀁 (Aa, 124)
    b"\xF0\x91\x80\x82", // 𑀂 (I, 125)
    b"\xF0\x91\x80\x83", // 𑀃 (Ii, 126)
    b"\xF0\x91\x80\x85", // 𑀅 (U, 127)
    // End of Sinhala Alphabet
    b"\xE0\xB6\xB1", // න (La, 128)
    b"\xE0\xB6\xB3", // ඳ (Va, 129)
    b"\xE0\xB6\xB4", // ප (Sha, 130)
    b"\xE0\xB6\xB5", // ඵ (Ssa, 131)
    b"\xE0\xB6\xB6", // බ (Sa, 132)
    b"\xE0\xB6\xB7", // භ (Ha, 133)
    // Glagolitic Alphabet
    b"\xE2\xB0\x80", // Ⰰ (Az, 134)
    b"\xE2\xB0\x81", // Ⰱ (Buky, 135)
    b"\xE2\xB0\x82", // Ⰲ (Vede, 136)
    b"\xE2\xB0\x83", // Ⰳ (Glagoli, 137)
    b"\xE2\xB0\x84", // Ⰴ (Dobro, 138)
    b"\xE2\xB0\x85", // Ⰵ (Yest, 139)
    b"\xE2\xB0\x86", // Ⰶ (Zhivete, 140)
    b"\xE2\xB0\x87", // Ⰷ (Zemlja, 141)
    b"\xE2\xB0\x88", // Ⰸ (Izhe, 142)
    b"\xE2\xB0\x89", // Ⰹ (Initial Izhe, 143)
    b"\xE2\xB0\x8A", // Ⰺ (I, 144)
    b"\xE2\xB0\x8B", // Ⰻ (Djerv, 145)
    b"\xE2\xB0\x8C", // Ⰼ (Kako, 146)
    b"\xE2\xB0\x8D", // Ⰽ (Ljudije, 147)
    b"\xE2\xB0\x8E", // Ⰾ (Myse, 148)
    b"\xE2\xB0\x8F", // Ⰿ (Nash, 149)
    b"\xE2\xB0\x90", // Ⱀ (On, 150)
    b"\xE2\xB0\x91", // Ⱁ (Pokoj, 151)
    b"\xE2\xB0\x92", // Ⱂ (Rtsy, 152)
    b"\xE2\xB0\x93", // Ⱃ (Slovo, 153)
    b"\xE2\xB0\x94", // Ⱄ (Tvrido, 154)
    b"\xE2\xB0\x95", // Ⱅ (Uku, 155)
    b"\xE2\xB0\x96", // Ⱆ (Fert, 156)
    b"\xE2\xB0\x97", // Ⱇ (Xrivi, 157)
    b"\xE2\xB0\x98", // Ⱈ (Ot, 158)
    b"\xE2\xB0\x99", // Ⱉ (Cy, 159)
    b"\xE2\xB0\x9A", // Ⱊ (Shcha, 160)
    b"\xE2\xB0\x9B", // Ⱋ (Er, 161)
    b"\xE2\xB0\x9C", // Ⱌ (Yeru, 162)
    b"\xE2\xB0\x9D", // Ⱍ (Small Yer, 163)
    b"\xE2\xB0\x9E", // Ⱎ (Yo, 164)
    b"\xE2\xB0\x9F", // Ⱏ (Yu, 165)
    b"\xE2\xB0\xA0", // Ⱐ (Ja, 166)
    // Thai Alphabet
    b"\xE0\xB8\x81", // ก (Ko Kai, 167)
    b"\xE0\xB8\x82", // ข (Kho Khai, 168)
    b"\xE0\xB8\x83", // ฃ (Kho Khuat, 169)
    b"\xE0\xB8\x84", // ค (Kho Khon, 170)
    b"\xE0\xB8\x85", // ฅ (Kho Rakhang, 171)
    b"\xE0\xB8\x86", // ฆ (Kho Khwai, 172)
    b"\xE0\xB8\x87", // ง (Ngo Ngu, 173)
    b"\xE0\xB8\x88", // จ (Cho Chan, 174)
    b"\xE0\xB8\x89", // ฉ (Cho Ching, 175)
    b"\xE0\xB8\x8A", // ช (Cho Chang, 176)
    b"\xE0\xB8\x8B", // ซ (So So, 177)
    b"\xE0\xB8\x8C", // ฌ (Cho Choe, 178)
    b"\xE0\xB8\x8D", // ญ (Yo Ying, 179)
    b"\xE0\xB8\x8E", // ฎ (Do Chada, 180)
    b"\xE0\xB8\x8F", // ฏ (To Patak, 181)
    b"\xE0\xB8\x90", // ฐ (Tho Than, 182)
    b"\xE0\xB8\x91", // ฑ (Tho Nangmontho, 183)
    b"\xE0\xB8\x92", // ฒ (Tho Phuthao, 184)
    b"\xE0\xB8\x93", // ณ (No Nen, 185)
    b"\xE0\xB8\x94", // ด (Do Dek, 186)
    b"\xE0\xB8\x95", // ต (To Tao, 187)
    b"\xE0\xB8\x96", // ถ (Tho Thung, 188)
    b"\xE0\xB8\x97", // ท (Tho Thahan, 189)
    b"\xE0\xB8\x98", // ธ (Tho Thong, 190)
    b"\xE0\xB8\x99", // น (No Nu, 191)
    b"\xE0\xB8\x9A", // บ (Bo Baimai, 192)
    b"\xE0\xB8\x9B", // ป (Po Pla, 193)
    b"\xE0\xB8\x9C", // ผ (Pho Phung, 194)
    b"\xE0\xB8\x9D", // ฝ (Fo Fa, 195)
    b"\xE0\xB8\x9E", // พ (Pho Phan, 196)
    b"\xE0\xB8\x9F", // ฟ (Fo Fan, 197)
    b"\xE0\xB8\xA0", // ภ (Pho Samphao, 198)
    b"\xE0\xB8\xA1", // ม (Mo Ma, 199)
    b"\xE0\xB8\xA2", // ย (Yo Yak, 200)
    b"\xE0\xB8\xA3", // ร (Ro Rua, 201)
    b"\xE0\xB8\xA5", // ล (Lo Ling, 202)
    b"\xE0\xB8\xA7", // ว (Wo Waen, 203)
    b"\xE0\xB8\xA8", // ศ (So Sala, 204)
    b"\xE0\xB8\xA9", // ษ (So Rusi, 205)
    b"\xE0\xB8\xAA", // ส (So Sua, 206)
    b"\xE0\xB8\xAB", // ห (Ho Hip, 207)
    b"\xE0\xB8\xAC", // ฬ (Lo Chula, 208)
    b"\xE0\xB8\xAD", // อ (O Ang, 209)
    b"\xE0\xB8\xAE", // ฮ (Ho Nokhuk, 210)
    // Hangul Alphabet (Korean)
    b"\xE3\x84\xB1", // ㄱ (Giyeok, 211)
    b"\xE3\x84\xB4", // ㄴ (Nieun, 212)
    b"\xE3\x84\xB7", // ㄷ (Digeut, 213)
    b"\xE3\x84\xB9", // ㄹ (Rieul, 214)
    b"\xE3\x85\x81", // ㅁ (Mieum, 215)
    b"\xE3\x85\x82", // ㅂ (Bieup, 216)
    b"\xE3\x85\x85", // ㅅ (Siot, 217)
    b"\xE3\x85\x87", // ㅇ (Ieung, 218)
    b"\xE3\x85\x88", // ㅈ (Jieut, 219)
    b"\xE3\x85\x8A", // ㅊ (Chieut, 220)
    b"\xE3\x85\x8B", // ㅋ (Kieuk, 221)
    b"\xE3\x85\x8C", // ㅌ (Tieut, 222)
    b"\xE3\x85\x8D", // ㅍ (Pieup, 223)
    b"\xE3\x85\x8E", // ㅎ (Hieut, 224)
    // Hangul Vowels
    b"\xE3\x85\x8F", // ㅏ (A, 225)
    b"\xE3\x85\x90", // ㅐ (Ae, 226)
    b"\xE3\x85\x91", // ㅑ (Ya, 227)
    b"\xE3\x85\x92", // ㅒ (Yae, 228)
    b"\xE3\x85\x93", // ㅓ (Eo, 229)
    b"\xE3\x85\x94", // ㅔ (E, 230)
    b"\xE3\x85\x95", // ㅕ (Yeo, 231)
    b"\xE3\x85\x96", // ㅖ (Ye, 232)
    b"\xE3\x85\x97", // ㅗ (O, 233)
    b"\xE3\x85\x98", // ㅘ (Wa, 234)
    b"\xE3\x85\x99", // ㅙ (Wae, 235)
    b"\xE3\x85\x9A", // ㅚ (Oe, 236)
    b"\xE3\x85\x9B", // ㅛ (Yo, 237)
    b"\xE3\x85\x9C", // ㅜ (U, 238)
    b"\xE3\x85\x9D", // ㅝ (Weo, 239)
    b"\xE3\x85\x9E", // ㅞ (We, 240)
    b"\xE3\x85\x9F", // ㅟ (Wi, 241)
    b"\xE3\x85\xA0", // ㅠ (Yu, 242)
    b"\xE3\x85\xA1", // ㅡ (Eu, 243)
    b"\xE3\x85\xA2", // ㅢ (Ui, 244)
    b"\xE3\x85\xA3", // ㅣ (I, 245)
    // Ethiopic Alphabet
    b"\xE1\x8A\xA0", // አ (Glottal A, 246)
    b"\xE1\x8A\xA1", // ኡ (Glottal U, 247)
    b"\xE1\x8A\xA2", // ኢ (Glottal I, 248)
    b"\xE1\x8A\xA3", // ኣ (Glottal Aa, 249)
    b"\xE1\x8A\xA4", // ኤ (Glottal E, 250)
    b"\xE1\x8A\xA5", // እ (Glottal Ie, 251)
    b"\xE1\x8A\xA6", // ኦ (Glottal O, 252)
    b"\xE1\x8A\xA7", // ኧ (Glottal Wa, 253)
    b"\xE1\x8B\x88", // ወ (Wa, 254)
    b"\xE1\x8B\x89", // ዉ (Wu, 255)
    b"\xE1\x8B\x8A", // ዊ (Wi, 256)
    b"\xE1\x8B\x8B", // ዋ (Waa, 257)
    b"\xE1\x8B\x8C", // ዌ (We, 258)
    b"\xE1\x8B\x8D", // ው (Wye, 259)
    b"\xE1\x8B\x8E", // ዎ (Wo, 260)
    b"\xE1\x8A\xB0", // ኰ (Ko, 261)
    b"\xE1\x8A\xB1", // ኱ (Ku, 262)
    b"\xE1\x8A\xB2", // ኲ (Ki, 263)
    b"\xE1\x8A\xB3", // ኳ (Kua, 264)
    b"\xE1\x8A\xB4", // ኴ (Ke, 265)
    b"\xE1\x8A\xB5", // ኵ (Kwe, 266)
    b"\xE1\x8A\xB6", // ኶ (Ko, 267)
    b"\xE1\x8C\x90", // ጐ (Go, 268)
    b"\xE1\x8C\x91", // ጑ (Gu, 269)
    b"\xE1\x8C\x92", // ጒ (Gi, 270)
    b"\xE1\x88\x98", // መ (Gua, 271)
    b"\xE1\x8C\x94", // ጔ (Ge, 272)
    b"\xE1\x8C\x95", // ጕ (Gwe, 273)
    b"\xE1\x8C\x96", // ጖ (Go, 274)
    // Devanagari Alphabet
    b"\xE0\xA4\x85", // अ (A, 275)
    b"\xE0\xA4\x86", // आ (Aa, 276)
    b"\xE0\xA4\x87", // इ (I, 277)
    b"\xE0\xA4\x88", // ई (Ii, 278)
    b"\xE0\xA4\x89", // उ (U, 279)
    b"\xE0\xA4\x8A", // ऊ (Uu, 280)
    b"\xE0\xA4\x8B", // ऋ (R, 281)
    b"\xE0\xA4\x8F", // ए (E, 282)
    b"\xE0\xA4\x90", // ऐ (Ai, 283)
    b"\xE0\xA4\x93", // ओ (O, 284)
    b"\xE0\xA4\x94", // औ (Au, 285)
    b"\xE0\xA4\x95", // क (Ka, 286)
    b"\xE0\xA4\x96", // ख (Kha, 287)
    b"\xE0\xA4\x97", // ग (Ga, 288)
    b"\xE0\xA4\x98", // घ (Gha, 289)
    b"\xE0\xA4\x99", // ङ (Nga, 290)
    b"\xE0\xA4\x9A", // च (Cha, 291)
    b"\xE0\xA4\x9B", // छ (Chha, 292)
    b"\xE0\xA4\x9C", // ज (Ja, 293)
    b"\xE0\xA4\x9D", // झ (Jha, 294)
    b"\xE0\xA4\x9E", // ञ (Nya, 295)
    b"\xE0\xA4\x9F", // ट (Ta, 296)
    b"\xE0\xA4\xA0", // ठ (Tha, 297)
    b"\xE0\xA4\xA1", // ड (Da, 298)
    b"\xE0\xA4\xA2", // ढ (Dha, 299)
    b"\xE0\xA4\xA3", // ण (Na, 300)
    b"\xE0\xA4\xA4", // त (Ta, 301)
    b"\xE0\xA4\xA5", // थ (Tha, 302)
    b"\xE0\xA4\xA6", // द (Da, 303)
    b"\xE0\xA4\xA7", // ध (Dha, 304)
    b"\xE0\xA4\xA8", // न (Na, 305)
    b"\xE0\xA4\xAA", // प (Pa, 306)
    b"\xE0\xA4\xAB", // फ (Pha, 307)
    b"\xE0\xA4\xAC", // ब (Ba, 308)
    b"\xE0\xA4\xAD", // भ (Bha, 309)
    b"\xE0\xA4\xAE", // म (Ma, 310)
    b"\xE0\xA4\xAF", // य (Ya, 311)
    b"\xE0\xA4\xB0", // र (Ra, 312)
    b"\xE0\xA4\xB2", // ल (La, 313)
    b"\xE0\xA4\xB5", // व (Va, 314)
    b"\xE0\xA4\xB6", // श (Sha, 315)
    b"\xE0\xA4\xB7", // ष (Ssa, 316)
    b"\xE0\xA4\xB8", // स (Sa, 317)
    b"\xE0\xA4\xB9", // ह (Ha, 318)
    // Katakana Alphabet
    b"\xE3\x82\xA2", // ア (A, 319)
    b"\xE3\x82\xA4", // イ (I, 320)
    b"\xE3\x82\xA6", // ウ (U, 321)
    b"\xE3\x82\xA8", // エ (E, 322)
    b"\xE3\x82\xAA", // オ (O, 323)
    b"\xE3\x82\xAB", // カ (Ka, 324)
    b"\xE3\x82\xAD", // キ (Ki, 325)
    b"\xE3\x82\xAF", // ク (Ku, 326)
    b"\xE3\x82\xB1", // ケ (Ke, 327)
    b"\xE3\x82\xB3", // コ (Ko, 328)
    b"\xE3\x82\xB5", // サ (Sa, 329)
    b"\xE3\x82\xB7", // シ (Shi, 330)
    b"\xE3\x82\xB9", // ス (Su, 331)
    b"\xE3\x82\xBB", // セ (Se, 332)
    b"\xE3\x82\xBD", // ソ (So, 333)
    b"\xE3\x82\xBF", // タ (Ta, 334)
    b"\xE3\x83\x81", // チ (Chi, 335)
    b"\xE3\x83\x84", // ツ (Tsu, 336)
    b"\xE3\x83\x86", // テ (Te, 337)
    b"\xE3\x83\x88", // ト (To, 338)
    b"\xE3\x83\x8A", // ナ (Na, 339)
    b"\xE3\x83\x8B", // ニ (Ni, 340)
    b"\xE3\x83\x8C", // ヌ (Nu, 341)
    b"\xE3\x83\x8D", // ネ (Ne, 342)
    b"\xE3\x83\x8E", // ノ (No, 343)
    b"\xE3\x83\x8F", // ハ (Ha, 344)
    b"\xE3\x83\x92", // ヒ (Hi, 345)
    b"\xE3\x83\x95", // フ (Fu, 346)
    b"\xE3\x83\x98", // ヘ (He, 347)
    b"\xE3\x83\x9B", // ホ (Ho, 348)
    b"\xE3\x83\x9E", // マ (Ma, 349)
    b"\xE3\x83\x9F", // ミ (Mi, 350)
    b"\xE3\x83\xA0", // ム (Mu, 351)
    b"\xE3\x83\xA1", // メ (Me, 352)
    b"\xE3\x83\xA2", // モ (Mo, 353)
    b"\xE3\x83\xA4", // ヤ (Ya, 354)
    b"\xE3\x83\xA6", // ユ (Yu, 355)
    b"\xE3\x83\xA8", // ヨ (Yo, 356)
    b"\xE3\x83\xA9", // ラ (Ra, 357)
    b"\xE3\x83\xAA", // リ (Ri, 358)
    b"\xE3\x83\xAB", // ル (Ru, 359)
    b"\xE3\x83\xAC", // レ (Re, 360)
    b"\xE3\x83\xAD", // ロ (Ro, 361)
    b"\xE3\x83\xAF", // ワ (Wa, 362)
    b"\xE3\x83\xB2", // ヲ (Wo, 363)
    b"\xE3\x83\xB3", // ン (N, 364)
    // Tifinagh Alphabet
    b"\xE2\xB4\xB0", // ⴰ (Ya, 365)
    b"\xE2\xB4\xB1", // ⴱ (Yab, 366)
    b"\xE2\xB4\xB2", // ⴲ (Yabh, 367)
    b"\xE2\xB4\xB3", // ⴳ (Yag, 368)
    b"\xE2\xB4\xB4", // ⴴ (Yagh, 369)
    b"\xE2\xB4\xB5", // ⴵ (Yaj, 370)
    b"\xE2\xB4\xB6", // ⴶ (Yach, 371)
    b"\xE2\xB4\xB7", // ⴷ (Yad, 372)
    b"\xE2\xB4\xB8", // ⴸ (Yadh, 373)
    b"\xE2\xB4\xB9", // ⴹ (Yadh, emphatic, 374)
    b"\xE2\xB4\xBA", // ⴺ (Yaz, 375)
    b"\xE2\xB4\xBB", // ⴻ (Yazh, 376)
    b"\xE2\xB4\xBC", // ⴼ (Yaf, 377)
    b"\xE2\xB4\xBD", // ⴽ (Yak, 378)
    b"\xE2\xB4\xBE", // ⴾ (Yak, variant, 379)
    b"\xE2\xB4\xBF", // ⴿ (Yaq, 380)
    b"\xE2\xB5\x80", // ⵀ (Yah, 381)
    b"\xE2\xB5\x81", // ⵁ (Yahh, 382)
    b"\xE2\xB5\x82", // ⵂ (Yahl, 383)
    b"\xE2\xB5\x83", // ⵃ (Yahm, 384)
    b"\xE2\xB5\x84", // ⵄ (Yayn, 385)
    b"\xE2\xB5\x85", // ⵅ (Yakh, 386)
    b"\xE2\xB5\x86", // ⵆ (Yakl, 387)
    b"\xE2\xB5\x87", // ⵇ (Yahq, 388)
    b"\xE2\xB5\x88", // ⵈ (Yash, 389)
    b"\xE2\xB5\x89", // ⵉ (Yi, 390)
    b"\xE2\xB5\x8A", // ⵊ (Yij, 391)
    b"\xE2\xB5\x8B", // ⵋ (Yizh, 392)
    b"\xE2\xB5\x8C", // ⵌ (Yink, 393)
    b"\xE2\xB5\x8D", // ⵍ (Yal, 394)
    b"\xE2\xB5\x8E", // ⵎ (Yam, 395)
    b"\xE2\xB5\x8F", // ⵏ (Yan, 396)
    b"\xE2\xB5\x90", // ⵐ (Yang, 397)
    b"\xE2\xB5\x91", // ⵑ (Yany, 398)
    b"\xE2\xB5\x92", // ⵒ (Yap, 399)
    b"\xE2\xB5\x93", // ⵓ (Yu, 400)
    // Sinhala Alphabet
    b"\xE0\xB6\x85", // අ (A, 401)
    b"\xE0\xB6\x86", // ආ (Aa, 402)
    b"\xE0\xB6\x89", // ඉ (I, 403)
    b"\xE0\xB6\x8A", // ඊ (Ii, 404)
    b"\xE0\xB6\x8B", // උ (U, 405)
    b"\xE0\xB6\x8C", // ඌ (Uu, 406)
    b"\xE0\xB6\x8D", // ඍ (R, 407)
    b"\xE0\xB6\x8E", // ඎ (Rr, 408)
    b"\xE0\xB6\x8F", // ඏ (L, 409)
    b"\xE0\xB6\x90", // ඐ (Ll, 410)
    b"\xE0\xB6\x91", // එ (E, 411)
    b"\xE0\xB6\x92", // ඒ (Ee, 412)
    b"\xE0\xB6\x93", // ඓ (Ai, 413)
    b"\xE0\xB6\x94", // ඔ (O, 414)
    b"\xE0\xB6\x95", // ඕ (Oo, 415)
    b"\xE0\xB6\x96", // ඖ (Au, 416)
    b"\xE0\xB6\x9A", // ක (Ka, 417)
    b"\xE0\xB6\x9B", // ඛ (Kha, 418)
    b"\xE0\xB6\x9C", // ග (Ga, 419)
    b"\xE0\xB6\x9D", // ඝ (Gha, 420)
    b"\xE0\xB6\x9E", // ඞ (Nga, 421)
    b"\xE0\xB6\xA0", // ච (Cha, 422)
    b"\xE0\xB6\xA1", // ඡ (Chha, 423)
    b"\xE0\xB6\xA2", // ජ (Ja, 424)
    b"\xE0\xB6\xA3", // ඣ (Jha, 425)
    b"\xE0\xB6\xA4", // ඤ (Nya, 426)
    b"\xE0\xB6\xA7", // ට (Ta, 427)
    b"\xE0\xB6\xA5", // ඥ (Tha, 428)
    b"\xE0\xB6\xA6", // ඦ (Da, 429)
    b"\xE0\xB6\xA9", // ඩ (Dha, 430)
    b"\xE0\xB6\xA8", // ඨ (Na, 431)
    b"\xE0\xB6\xAA", // ඪ (Pa, 432)
    b"\xE0\xB6\xAB", // ණ (Pha, 433)
    b"\xE0\xB6\xAC", // ඬ (Ba, 434)
    b"\xE0\xB6\xAD", // ත (Bha, 435)
    b"\xE0\xB6\xAE", // ථ (Ma, 436)
    b"\xE0\xB6\xAF", // ද (Ya, 437)
    b"\xE0\xB6\xB0", // ධ (Ra, 438)
];

/// Returns the Unicode symbol as a Vec<u8> for a given netuid.
impl<T: Config> Pallet<T> {
    /// Human-readable UTF-8 name for `netuid` from the static name table.
    pub fn get_name_for_subnet(netuid: NetUid) -> Vec<u8> {
        SubnetIdentitiesV3::<T>::try_get(netuid)
            .and_then(|identity| {
                if !identity.subnet_name.is_empty() {
                    Ok(identity.subnet_name)
                } else {
                    Err(())
                }
            })
            .unwrap_or_else(|_| {
                match u16::from(netuid) {
                    0 => b"root".to_vec(),          // Τ (Upper case Tau)
                    1 => b"apex".to_vec(),          // α (Alpha)
                    2 => b"omron".to_vec(),         // β (Beta)
                    3 => b"templar".to_vec(),       // γ (Gamma)
                    4 => b"targon".to_vec(),        // δ (Delta)
                    5 => b"kaito".to_vec(),         // ε (Epsilon)
                    6 => b"infinite".to_vec(),      // ζ (Zeta)
                    7 => b"subvortex".to_vec(),     // η (Eta)
                    8 => b"ptn".to_vec(),           // θ (Theta)
                    9 => b"pretrain".to_vec(),      // ι (Iota)
                    10 => b"sturdy".to_vec(),       // κ (Kappa)
                    11 => b"dippy".to_vec(),        // λ (Lambda)
                    12 => b"horde".to_vec(),        // μ (Mu)
                    13 => b"dataverse".to_vec(),    // ν (Nu)
                    14 => b"palaidn".to_vec(),      // ξ (Xi)
                    15 => b"deval".to_vec(),        // ο (Omicron)
                    16 => b"bitads".to_vec(),       // π (Pi)
                    17 => b"3gen".to_vec(),         // ρ (Rho)
                    18 => b"cortex".to_vec(),       // σ (Sigma)
                    19 => b"inference".to_vec(),    // t (Tau)
                    20 => b"bitagent".to_vec(),     // υ (Upsilon)
                    21 => b"any-any".to_vec(),      // φ (Phi)
                    22 => b"meta".to_vec(),         // χ (Chi)
                    23 => b"social".to_vec(),       // ψ (Psi)
                    24 => b"omega".to_vec(),        // ω (Omega)
                    25 => b"protein".to_vec(),      // א (Aleph)
                    26 => b"alchemy".to_vec(),      // ב (Bet)
                    27 => b"compute".to_vec(),      // ג (Gimel)
                    28 => b"oracle".to_vec(),       // ד (Dalet)
                    29 => b"coldint".to_vec(),      // ה (He)
                    30 => b"bet".to_vec(),          // ו (Vav)
                    31 => b"naschain".to_vec(),     // ז (Zayin)
                    32 => b"itsai".to_vec(),        // ח (Het)
                    33 => b"ready".to_vec(),        // ט (Tet)
                    34 => b"mind".to_vec(),         // י (Yod)
                    35 => b"logic".to_vec(),        // ך (Final Kaf)
                    36 => b"automata".to_vec(),     // כ (Kaf)
                    37 => b"tuning".to_vec(),       // ל (Lamed)
                    38 => b"distributed".to_vec(),  // ם (Final Mem)
                    39 => b"edge".to_vec(),         // מ (Mem)
                    40 => b"chunk".to_vec(),        // ן (Final Nun)
                    41 => b"sportsensor".to_vec(),  // נ (Nun)
                    42 => b"masa".to_vec(),         // ס (Samekh)
                    43 => b"graphite".to_vec(),     // ע (Ayin)
                    44 => b"score".to_vec(),        // ף (Final Pe)
                    45 => b"gen42".to_vec(),        // פ (Pe)
                    46 => b"neural".to_vec(),       // ץ (Final Tsadi)
                    47 => b"condense".to_vec(),     // צ (Tsadi)
                    48 => b"nextplace".to_vec(),    // ק (Qof)
                    49 => b"automl".to_vec(),       // ר (Resh)
                    50 => b"audio".to_vec(),        // ש (Shin)
                    51 => b"celium".to_vec(),       // ת (Tav)
                    52 => b"dojo".to_vec(),         // ا (Alif)
                    53 => b"frontier".to_vec(),     // ب (Ba)
                    54 => b"safescan".to_vec(),     // ت (Ta)
                    55 => b"unknown".to_vec(),      // ث (Tha)
                    56 => b"gradients".to_vec(),    // ج (Jim)
                    57 => b"gaia".to_vec(),         // ح (Ha)
                    58 => b"dippy-speach".to_vec(), // خ (Kha)
                    59 => b"agent-arena".to_vec(),  // د (Dal)
                    60 => b"unknown".to_vec(),      // ذ (Dhal)
                    61 => b"red team".to_vec(),     // ر (Ra)
                    62 => b"agentao".to_vec(),      // ز (Zay)
                    63 => b"lean-in".to_vec(),      // س (Sin)
                    64 => b"chutes".to_vec(),       // ش (Shin)
                    65 => b"sad".to_vec(),
                    66 => b"dad".to_vec(),
                    67 => b"ta".to_vec(),
                    68 => b"dha".to_vec(),
                    69 => b"ain".to_vec(),
                    70 => b"ghayn".to_vec(),
                    71 => b"fa".to_vec(),
                    72 => b"qaf".to_vec(),
                    73 => b"kaf".to_vec(),
                    74 => b"lam".to_vec(),
                    75 => b"mim".to_vec(),
                    76 => b"nun".to_vec(),
                    77 => b"ha".to_vec(),
                    78 => b"waw".to_vec(),
                    79 => b"ya".to_vec(),
                    80 => b"alef".to_vec(),
                    81 => b"fehu".to_vec(),
                    82 => b"uruz".to_vec(),
                    83 => b"thurisaz".to_vec(),
                    84 => b"ansuz".to_vec(),
                    85 => b"raidho".to_vec(),
                    86 => b"kaunan".to_vec(),
                    87 => b"cyr_yeru".to_vec(),
                    88 => b"algiz".to_vec(),
                    89 => b"berkanan".to_vec(),
                    90 => b"ogham".to_vec(),
                    91 => b"beith".to_vec(),
                    92 => b"luis".to_vec(),
                    93 => b"fearn".to_vec(),
                    94 => b"sail".to_vec(),
                    95 => b"nion".to_vec(),
                    96 => b"forfeda".to_vec(),
                    97 => b"ani".to_vec(),
                    98 => b"bani".to_vec(),
                    99 => b"gani".to_vec(),
                    100 => b"doni".to_vec(),
                    101 => b"eni".to_vec(),
                    102 => b"vini".to_vec(),
                    103 => b"ayp".to_vec(),
                    104 => b"ben".to_vec(),
                    105 => b"gim".to_vec(),
                    106 => b"da".to_vec(),
                    107 => b"ech".to_vec(),
                    108 => b"za".to_vec(),
                    109 => b"armeni".to_vec(),
                    110 => b"grave".to_vec(),
                    111 => b"io".to_vec(),
                    112 => b"dje".to_vec(),
                    113 => b"gje".to_vec(),
                    114 => b"ie".to_vec(),
                    115 => b"dze".to_vec(),
                    116 => b"hard_sign".to_vec(),
                    117 => b"alfa".to_vec(),
                    118 => b"alfas".to_vec(),
                    119 => b"vida".to_vec(),        // Ⲃ (Vida, 119)
                    120 => b"vida_small".to_vec(),  // ⲃ (Small Vida, 120)
                    121 => b"gamma".to_vec(),       // Ⲅ (Gamma, 121)
                    122 => b"gamma_small".to_vec(), // ⲅ (Small Gamma, 122)
                    123 => b"brahmi_a".to_vec(),    // 𑀀 (A, 123)
                    124 => b"brahmi_aa".to_vec(),   // 𑀁 (Aa, 124)
                    125 => b"brahmi_i".to_vec(),    // 𑀂 (I, 125)
                    126 => b"brahmi_ii".to_vec(),   // 𑀃 (Ii, 126)
                    127 => b"brahmi_u".to_vec(),    // 𑀅 (U, 127)
                    128 => b"la".to_vec(),
                    129 => b"va".to_vec(),
                    130 => b"sha".to_vec(),
                    131 => b"ssa".to_vec(),
                    132 => b"sa".to_vec(),
                    133 => b"ha".to_vec(),
                    134 => b"glagolitic_az".to_vec(), // Ⰰ (Az, 134)
                    135 => b"glagolitic_buky".to_vec(), // Ⰱ (Buky, 135)
                    136 => b"glagolitic_vede".to_vec(), // Ⰲ (Vede, 136)
                    137 => b"glagolitic_glagoli".to_vec(), // Ⰳ (Glagoli, 137)
                    138 => b"glagolitic_dobro".to_vec(), // Ⰴ (Dobro, 138)
                    139 => b"glagolitic_yest".to_vec(), // Ⰵ (Yest, 139)
                    140 => b"glagolitic_zhivete".to_vec(), // Ⰶ (Zhivete, 140)
                    141 => b"glagolitic_zemlja".to_vec(), // Ⰷ (Zemlja, 141)
                    142 => b"glagolitic_izhe".to_vec(), // Ⰸ (Izhe, 142)
                    143 => b"glagolitic_initial_izhe".to_vec(), // Ⰹ (Initial Izhe, 143)
                    144 => b"glagolitic_i".to_vec(),  // Ⰺ (I, 144)
                    145 => b"glagolitic_djerv".to_vec(), // Ⰻ (Djerv, 145)
                    146 => b"glagolitic_kako".to_vec(), // Ⰼ (Kako, 146)
                    147 => b"glagolitic_ljudije".to_vec(), // Ⰽ (Ljudije, 147)
                    148 => b"glagolitic_myse".to_vec(), // Ⰾ (Myse, 148)
                    149 => b"glagolitic_nash".to_vec(), // Ⰿ (Nash, 149)
                    150 => b"glagolitic_on".to_vec(), // Ⱀ (On, 150)
                    151 => b"glagolitic_pokoj".to_vec(), // Ⱁ (Pokoj, 151)
                    152 => b"glagolitic_rtsy".to_vec(), // Ⱂ (Rtsy, 152)
                    153 => b"glagolitic_slovo".to_vec(), // Ⱃ (Slovo, 153)
                    154 => b"glagolitic_tvrido".to_vec(), // Ⱄ (Tvrido, 154)
                    155 => b"glagolitic_uku".to_vec(), // Ⱅ (Uku, 155)
                    156 => b"glagolitic_fert".to_vec(), // Ⱆ (Fert, 156)
                    157 => b"glagolitic_xrivi".to_vec(), // Ⱇ (Xrivi, 157)
                    158 => b"glagolitic_ot".to_vec(), // Ⱈ (Ot, 158)
                    159 => b"glagolitic_cy".to_vec(), // Ⱉ (Cy, 159)
                    160 => b"glagolitic_shcha".to_vec(), // Ⱊ (Shcha, 160)
                    161 => b"glagolitic_er".to_vec(), // Ⱋ (Er, 161)
                    162 => b"glagolitic_yeru".to_vec(), // Ⱌ (Yeru, 162)
                    163 => b"glagolitic_small_yer".to_vec(), // Ⱍ (Small Yer, 163)
                    164 => b"glagolitic_yo".to_vec(), // Ⱎ (Yo, 164)
                    165 => b"glagolitic_yu".to_vec(), // Ⱏ (Yu, 165)
                    166 => b"glagolitic_ja".to_vec(), // Ⱐ (Ja, 166)
                    167 => b"thai_ko_kai".to_vec(),   // ก (Ko Kai, 167)
                    168 => b"thai_kho_khai".to_vec(), // ข (Kho Khai, 168)
                    169 => b"thai_kho_khuat".to_vec(), // ฃ (Kho Khuat, 169)
                    170 => b"thai_kho_khon".to_vec(), // ค (Kho Khon, 170)
                    171 => b"thai_kho_rakhang".to_vec(), // ฅ (Kho Rakhang, 171)
                    172 => b"thai_kho_khwai".to_vec(), // ฆ (Kho Khwai, 172)
                    173 => b"thai_ngo_ngu".to_vec(),  // ง (Ngo Ngu, 173)
                    174 => b"thai_cho_chan".to_vec(), // จ (Cho Chan, 174)
                    175 => b"thai_cho_ching".to_vec(), // ฉ (Cho Ching, 175)
                    176 => b"thai_cho_chang".to_vec(), // ช (Cho Chang, 176)
                    177 => b"thai_so_so".to_vec(),    // ซ (So So, 177)
                    178 => b"thai_cho_choe".to_vec(), // ฌ (Cho Choe, 178)
                    179 => b"thai_yo_ying".to_vec(),  // ญ (Yo Ying, 179)
                    180 => b"thai_do_chada".to_vec(), // ฎ (Do Chada, 180)
                    181 => b"thai_to_patak".to_vec(), // ฏ (To Patak, 181)
                    182 => b"thai_tho_than".to_vec(), // ฐ (Tho Than, 182)
                    183 => b"thai_tho_nangmontho".to_vec(), // ฑ (Tho Nangmontho, 183)
                    184 => b"thai_tho_phuthao".to_vec(), // ฒ (Tho Phuthao, 184)
                    185 => b"thai_no_nen".to_vec(),   // ณ (No Nen, 185)
                    186 => b"thai_do_dek".to_vec(),   // ด (Do Dek, 186)
                    187 => b"thai_to_tao".to_vec(),   // ต (To Tao, 187)
                    188 => b"thai_tho_thung".to_vec(), // ถ (Tho Thung, 188)
                    189 => b"thai_tho_thahan".to_vec(), // ท (Tho Thahan, 189)
                    190 => b"thai_tho_thong".to_vec(), // ธ (Tho Thong, 190)
                    191 => b"thai_no_nu".to_vec(),    // น (No Nu, 191)
                    192 => b"thai_bo_baimai".to_vec(), // บ (Bo Baimai, 192)
                    193 => b"thai_po_pla".to_vec(),   // ป (Po Pla, 193)
                    194 => b"thai_pho_phung".to_vec(), // ผ (Pho Phung, 194)
                    195 => b"thai_fo_fa".to_vec(),    // ฝ (Fo Fa, 195)
                    196 => b"thai_pho_phan".to_vec(), // พ (Pho Phan, 196)
                    197 => b"thai_fo_fan".to_vec(),   // ฟ (Fo Fan, 197)
                    198 => b"thai_pho_samphao".to_vec(), // ภ (Pho Samphao, 198)
                    199 => b"thai_mo_ma".to_vec(),    // ม (Mo Ma, 199)
                    200 => b"thai_yo_yak".to_vec(),   // ย (Yo Yak, 200)
                    201 => b"thai_ro_rua".to_vec(),   // ร (Ro Rua, 201)
                    202 => b"thai_lo_ling".to_vec(),  // ล (Lo Ling, 202)
                    203 => b"thai_wo_waen".to_vec(),  // ว (Wo Waen, 203)
                    204 => b"thai_so_sala".to_vec(),  // ศ (So Sala, 204)
                    205 => b"thai_so_rusi".to_vec(),  // ษ (So Rusi, 205)
                    206 => b"thai_so_sua".to_vec(),   // ส (So Sua, 206)
                    207 => b"thai_ho_hip".to_vec(),   // ห (Ho Hip, 207)
                    208 => b"thai_lo_chula".to_vec(), // ฬ (Lo Chula, 208)
                    209 => b"thai_o_ang".to_vec(),    // อ (O Ang, 209)
                    210 => b"thai_ho_nokhuk".to_vec(), // ฮ (Ho Nokhuk, 210)
                    211 => b"hangul_giyeok".to_vec(), // ㄱ (Giyeok, 211)
                    212 => b"hangul_nieun".to_vec(),  // ㄴ (Nieun, 212)
                    213 => b"hangul_digeut".to_vec(), // ㄷ (Digeut, 213)
                    214 => b"hangul_rieul".to_vec(),  // ㄹ (Rieul, 214)
                    215 => b"hangul_mieum".to_vec(),  // ㅁ (Mieum, 215)
                    216 => b"hangul_bieup".to_vec(),  // ㅂ (Bieup, 216)
                    217 => b"hangul_siot".to_vec(),   // ㅅ (Siot, 217)
                    218 => b"hangul_ieung".to_vec(),  // ㅇ (Ieung, 218)
                    219 => b"hangul_jieut".to_vec(),  // ㅈ (Jieut, 219)
                    220 => b"hangul_chieut".to_vec(), // ㅊ (Chieut, 220)
                    221 => b"hangul_kieuk".to_vec(),  // ㅋ (Kieuk, 221)
                    222 => b"hangul_tieut".to_vec(),  // ㅌ (Tieut, 222)
                    223 => b"hangul_pieup".to_vec(),  // ㅍ (Pieup, 223)
                    224 => b"hangul_hieut".to_vec(),  // ㅎ (Hieut, 224)
                    225 => b"hangul_a".to_vec(),      // ㅏ (A, 225)
                    226 => b"hangul_ae".to_vec(),     // ㅐ (Ae, 226)
                    227 => b"hangul_ya".to_vec(),     // ㅑ (Ya, 227)
                    228 => b"hangul_yae".to_vec(),    // ㅒ (Yae, 228)
                    229 => b"hangul_eo".to_vec(),     // ㅓ (Eo, 229)
                    230 => b"hangul_e".to_vec(),      // ㅔ (E, 230)
                    231 => b"hangul_yeo".to_vec(),    // ㅕ (Yeo, 231)
                    232 => b"hangul_ye".to_vec(),     // ㅖ (Ye, 232)
                    233 => b"hangul_o".to_vec(),      // ㅗ (O, 233)
                    234 => b"hangul_wa".to_vec(),     // ㅘ (Wa, 234)
                    235 => b"hangul_wae".to_vec(),    // ㅙ (Wae, 235)
                    236 => b"hangul_oe".to_vec(),     // ㅚ (Oe, 236)
                    237 => b"hangul_yo".to_vec(),     // ㅛ (Yo, 237)
                    238 => b"hangul_u".to_vec(),      // ㅜ (U, 238)
                    239 => b"hangul_weo".to_vec(),    // ㅝ (Weo, 239)
                    240 => b"hangul_we".to_vec(),     // ㅞ (We, 240)
                    241 => b"hangul_wi".to_vec(),     // ㅟ (Wi, 241)
                    242 => b"hangul_yu".to_vec(),     // ㅠ (Yu, 242)
                    243 => b"hangul_eu".to_vec(),     // ㅡ (Eu, 243)
                    244 => b"hangul_ui".to_vec(),     // ㅢ (Ui, 244)
                    245 => b"hangul_i".to_vec(),      // ㅣ (I, 245)
                    246 => b"ethiopic_glottal_a".to_vec(), // አ (Glottal A, 246)
                    247 => b"ethiopic_glottal_u".to_vec(), // ኡ (Glottal U, 247)
                    248 => b"ethiopic_glottal_i".to_vec(), // ኢ (Glottal I, 248)
                    249 => b"ethiopic_glottal_aa".to_vec(), // ኣ (Glottal Aa, 249)
                    250 => b"ethiopic_glottal_e".to_vec(), // ኤ (Glottal E, 250)
                    251 => b"ethiopic_glottal_ie".to_vec(), // እ (Glottal Ie, 251)
                    252 => b"ethiopic_glottal_o".to_vec(), // ኦ (Glottal O, 252)
                    253 => b"ethiopic_glottal_wa".to_vec(), // ኧ (Glottal Wa, 253)
                    254 => b"ethiopic_wa".to_vec(),   // ወ (Wa, 254)
                    255 => b"ethiopic_wu".to_vec(),   // ዉ (Wu, 255)
                    256 => b"ethiopic_wi".to_vec(),   // ዊ (Wi, 256)
                    257 => b"ethiopic_waa".to_vec(),  // ዋ (Waa, 257)
                    258 => b"ethiopic_we".to_vec(),   // ዌ (We, 258)
                    259 => b"ethiopic_wye".to_vec(),  // ው (Wye, 259)
                    260 => b"ethiopic_wo".to_vec(),   // ዎ (Wo, 260)
                    261 => b"ethiopic_ko".to_vec(),   // ኰ (Ko, 261)
                    262 => b"ethiopic_ku".to_vec(),   // ኱ (Ku, 262)
                    263 => b"ethiopic_ki".to_vec(),   // ኲ (Ki, 263)
                    264 => b"ethiopic_kua".to_vec(),  // ኳ (Kua, 264)
                    265 => b"ethiopic_ke".to_vec(),   // ኴ (Ke, 265)
                    266 => b"ethiopic_kwe".to_vec(),  // ኵ (Kwe, 266)
                    267 => b"ethiopic_ko_alt".to_vec(), // ኶ (Ko, 267)
                    268 => b"ethiopic_go".to_vec(),   // ጐ (Go, 268)
                    269 => b"ethiopic_gu".to_vec(),   // ጑ (Gu, 269)
                    270 => b"ethiopic_gi".to_vec(),   // ጒ (Gi, 270)
                    271 => b"ethiopic_gua".to_vec(),  // መ (Gua, 271)
                    272 => b"ethiopic_ge".to_vec(),   // ጔ (Ge, 272)
                    273 => b"ethiopic_gwe".to_vec(),  // ጕ (Gwe, 273)
                    274 => b"ethiopic_go_alt".to_vec(), // ጖ (Go, 274)
                    275 => b"devanagari_a".to_vec(),  // अ (A, 275)
                    276 => b"devanagari_aa".to_vec(), // आ (Aa, 276)
                    277 => b"devanagari_i".to_vec(),  // इ (I, 277)
                    278 => b"devanagari_ii".to_vec(), // ई (Ii, 278)
                    279 => b"devanagari_u".to_vec(),  // उ (U, 279)
                    280 => b"devanagari_uu".to_vec(), // ऊ (Uu, 280)
                    281 => b"devanagari_r".to_vec(),  // ऋ (R, 281)
                    282 => b"devanagari_e".to_vec(),  // ए (E, 282)
                    283 => b"devanagari_ai".to_vec(), // ऐ (Ai, 283)
                    284 => b"devanagari_o".to_vec(),  // ओ (O, 284)
                    285 => b"devanagari_au".to_vec(), // औ (Au, 285)
                    286 => b"devanagari_ka".to_vec(), // क (Ka, 286)
                    287 => b"devanagari_kha".to_vec(), // ख (Kha, 287)
                    288 => b"devanagari_ga".to_vec(), // ग (Ga, 288)
                    289 => b"devanagari_gha".to_vec(), // घ (Gha, 289)
                    290 => b"devanagari_nga".to_vec(), // ङ (Nga, 290)
                    291 => b"devanagari_cha".to_vec(), // च (Cha, 291)
                    292 => b"devanagari_chha".to_vec(), // छ (Chha, 292)
                    293 => b"devanagari_ja".to_vec(), // ज (Ja, 293)
                    294 => b"devanagari_jha".to_vec(), // झ (Jha, 294)
                    295 => b"devanagari_nya".to_vec(), // ञ (Nya, 295)
                    296 => b"devanagari_ta".to_vec(), // ट (Ta, 296)
                    297 => b"devanagari_tha".to_vec(), // ठ (Tha, 297)
                    298 => b"devanagari_da".to_vec(), // ड (Da, 298)
                    299 => b"devanagari_dha".to_vec(), // ढ (Dha, 299)
                    300 => b"devanagari_na".to_vec(), // ण (Na, 300)
                    301 => b"devanagari_ta_alt".to_vec(), // त (Ta, 301)
                    302 => b"devanagari_tha_alt".to_vec(), // थ (Tha, 302)
                    303 => b"devanagari_da_alt".to_vec(), // द (Da, 303)
                    304 => b"devanagari_dha_alt".to_vec(), // ध (Dha, 304)
                    305 => b"devanagari_na_alt".to_vec(), // न (Na, 305)
                    306 => b"devanagari_pa".to_vec(), // प (Pa, 306)
                    307 => b"devanagari_pha".to_vec(), // फ (Pha, 307)
                    308 => b"devanagari_ba".to_vec(), // ब (Ba, 308)
                    309 => b"devanagari_bha".to_vec(), // भ (Bha, 309)
                    310 => b"devanagari_ma".to_vec(), // म (Ma, 310)
                    311 => b"devanagari_ya".to_vec(), // य (Ya, 311)
                    312 => b"devanagari_ra".to_vec(), // र (Ra, 312)
                    313 => b"devanagari_la".to_vec(), // ल (La, 313)
                    314 => b"devanagari_va".to_vec(), // व (Va, 314)
                    315 => b"devanagari_sha".to_vec(), // श (Sha, 315)
                    316 => b"devanagari_ssa".to_vec(), // ष (Ssa, 316)
                    317 => b"devanagari_sa".to_vec(), // स (Sa, 317)
                    318 => b"devanagari_ha".to_vec(), // ह (Ha, 318)
                    319 => b"katakana_a".to_vec(),    // ア (A, 319)
                    320 => b"kana_i".to_vec(),
                    321 => b"kana_u".to_vec(),
                    322 => b"kana_e".to_vec(),
                    323 => b"kana_o".to_vec(),
                    324 => b"kana_a".to_vec(),
                    325 => b"kana_ki".to_vec(),
                    326 => b"kana_ku".to_vec(),
                    327 => b"kana_ke".to_vec(),
                    328 => b"kana_ko".to_vec(),
                    329 => b"kana_sa".to_vec(),
                    330 => b"kana_shi".to_vec(),
                    331 => b"kana_su".to_vec(),
                    332 => b"kana_se".to_vec(),
                    333 => b"kana_so".to_vec(),
                    334 => b"kana_ta".to_vec(),
                    335 => b"kana_chi".to_vec(),
                    336 => b"kana_tsu".to_vec(),
                    337 => b"kana_te".to_vec(),
                    338 => b"kana_to".to_vec(),
                    339 => b"kana_na".to_vec(),
                    340 => b"kana_ni".to_vec(),
                    341 => b"kana_nu".to_vec(),
                    342 => b"kana_ne".to_vec(),
                    343 => b"kana_no".to_vec(),
                    344 => b"kana_ha".to_vec(),
                    345 => b"kana_hi".to_vec(),
                    346 => b"kana_fu".to_vec(),
                    347 => b"kana_he".to_vec(),
                    348 => b"kana_ho".to_vec(),
                    349 => b"kana_ma".to_vec(),
                    350 => b"kana_mi".to_vec(),
                    351 => b"kana_mu".to_vec(),
                    352 => b"kana_me".to_vec(),
                    353 => b"kana_mo".to_vec(),
                    354 => b"kana_ya".to_vec(),
                    355 => b"kana_yu".to_vec(),
                    356 => b"kana_yo".to_vec(),
                    357 => b"kana_ra".to_vec(),
                    358 => b"kana_ri".to_vec(),
                    359 => b"kana_ru".to_vec(),
                    360 => b"kana_re".to_vec(),
                    361 => b"kana_ro".to_vec(),
                    362 => b"kana_wa".to_vec(),
                    363 => b"kana_wo".to_vec(),
                    364 => b"kana_n".to_vec(),
                    365 => b"ya".to_vec(),
                    366 => b"yab".to_vec(),
                    367 => b"yabh".to_vec(),
                    368 => b"yag".to_vec(),
                    369 => b"yagh".to_vec(),
                    370 => b"yaj".to_vec(),
                    371 => b"yach".to_vec(),
                    372 => b"yad".to_vec(),
                    373 => b"yadh".to_vec(),
                    374 => b"yadhe".to_vec(),
                    375 => b"yaz".to_vec(),
                    376 => b"yazh".to_vec(),
                    377 => b"yaf".to_vec(),
                    378 => b"yak".to_vec(),
                    379 => b"yakv".to_vec(),
                    380 => b"yaq".to_vec(),
                    381 => b"yah".to_vec(),
                    382 => b"yahh".to_vec(),
                    383 => b"yahl".to_vec(),
                    384 => b"yahm".to_vec(),
                    385 => b"yayn".to_vec(),
                    386 => b"yakh".to_vec(),
                    387 => b"yakl".to_vec(),
                    388 => b"yahq".to_vec(),
                    389 => b"yash".to_vec(),
                    390 => b"yi".to_vec(),
                    391 => b"yij".to_vec(),
                    392 => b"yizh".to_vec(),
                    393 => b"yink".to_vec(),
                    394 => b"yal".to_vec(),
                    395 => b"yam".to_vec(),
                    396 => b"yan".to_vec(),
                    397 => b"yang".to_vec(),
                    398 => b"yany".to_vec(),
                    399 => b"yap".to_vec(),
                    400 => b"yu".to_vec(),
                    401 => b"a".to_vec(),
                    402 => b"aa".to_vec(),
                    403 => b"i".to_vec(),
                    404 => b"ii".to_vec(),
                    405 => b"u".to_vec(),
                    406 => b"uu".to_vec(),
                    407 => b"r".to_vec(),
                    408 => b"rr".to_vec(),
                    409 => b"l".to_vec(),
                    410 => b"ll".to_vec(),
                    411 => b"e".to_vec(),
                    412 => b"ee".to_vec(),
                    413 => b"ai".to_vec(),
                    414 => b"o".to_vec(),
                    415 => b"oo".to_vec(),
                    416 => b"au".to_vec(),
                    417 => b"ka".to_vec(),
                    418 => b"kha".to_vec(),
                    419 => b"ga".to_vec(),
                    420 => b"gha".to_vec(),
                    421 => b"nga".to_vec(),
                    422 => b"cha".to_vec(),
                    423 => b"chha".to_vec(),
                    424 => b"ja".to_vec(),
                    425 => b"jha".to_vec(),
                    426 => b"nya".to_vec(),
                    427 => b"ta".to_vec(),
                    428 => b"tha".to_vec(),
                    429 => b"da".to_vec(),
                    430 => b"dha".to_vec(),
                    431 => b"na".to_vec(),
                    432 => b"pa".to_vec(),
                    433 => b"pha".to_vec(),
                    434 => b"ba".to_vec(),
                    435 => b"bha".to_vec(),
                    436 => b"ma".to_vec(),
                    437 => b"ya".to_vec(),
                    438 => b"ra".to_vec(),
                    _ => b"unknown".to_vec(),
                }
            })
    }

    /// Default token symbol bytes for `netuid` from [`SYMBOLS`] (falls back to root symbol).
    pub fn get_symbol_for_subnet(netuid: NetUid) -> Vec<u8> {
        SYMBOLS
            .get(u16::from(netuid) as usize)
            .unwrap_or(&DEFAULT_SYMBOL)
            .to_vec()
    }

    /// First unused symbol from [`SYMBOLS`], preferring the netuid-default slot.
    pub fn get_next_available_symbol(netuid: NetUid) -> Vec<u8> {
        let used_symbols: BTreeSet<Vec<u8>> = TokenSymbol::<T>::iter_values().collect();

        // We first try the default strategy of using the subnet id to get the symbol.
        let symbol = Self::get_symbol_for_subnet(netuid);
        if !used_symbols.contains(&symbol) {
            return symbol;
        }

        // If it is already taken, we try to get the next available symbol.
        let available_symbol = SYMBOLS
            .iter()
            .skip(1) // Skip the root symbol
            .find(|s| !used_symbols.contains(&s[..]))
            .map(|s| s.to_vec());

        if available_symbol.is_none() {
            log::warn!(
                "All available symbols have been exhausted for netuid: {netuid:?}. Using default symbol."
            );
        }

        // If we have exhausted all symbols, we use the default symbol.
        available_symbol.unwrap_or(DEFAULT_SYMBOL.to_vec())
    }

    /// Error unless `symbol` is in the static [`SYMBOLS`] table (excluding root).
    pub fn ensure_symbol_exists(symbol: &[u8]) -> DispatchResult {
        if !SYMBOLS.iter().skip(1).any(|s| s == &symbol) {
            return Err(Error::<T>::SymbolDoesNotExist.into());
        }

        Ok(())
    }

    /// Error if `symbol` is already assigned in [`TokenSymbol`].
    pub fn ensure_symbol_available(symbol: &[u8]) -> DispatchResult {
        if TokenSymbol::<T>::iter_values().any(|s| s == symbol) {
            return Err(Error::<T>::SymbolAlreadyInUse.into());
        }

        Ok(())
    }
}
