// Registration SHA256 -> Keccak-256. Public challenge and nonce only.
// Bounded memory: the host passes 123 prefix bytes, target, and one result.
__constant uint SHA_K[64] = {
0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2};
__constant ulong KECCAK_RC[24] = {
0x0000000000000001UL,0x0000000000008082UL,0x800000000000808aUL,0x8000000080008000UL,
0x000000000000808bUL,0x0000000080000001UL,0x8000000080008081UL,0x8000000000008009UL,
0x000000000000008aUL,0x0000000000000088UL,0x0000000080008009UL,0x000000008000000aUL,
0x000000008000808bUL,0x800000000000008bUL,0x8000000000008089UL,0x8000000000008003UL,
0x8000000000008002UL,0x8000000000000080UL,0x000000000000800aUL,0x800000008000000aUL,
0x8000000080008081UL,0x8000000000008080UL,0x0000000080000001UL,0x8000000080008008UL};
__constant uint KECCAK_RHO[25] = {0,1,62,28,27,36,44,6,55,20,3,10,43,25,39,41,45,15,21,8,18,2,61,56,14};
uint rr(uint x, uint n) { return (x >> n) | (x << (32 - n)); }
ulong rl(ulong x, uint n) { return n == 0 ? x : (x << n) | (x >> (64 - n)); }
void make_seal(__global const uchar *prefix, ulong nonce, uchar *seal) {
    uchar message[192];
    for (uint i=0;i<192;i++) message[i]=0;
    for (uint i=0;i<123;i++) message[i]=prefix[i];
    for (uint i=0;i<8;i++) message[123+i]=(uchar)(nonce>>(8*i));
    message[131]=0x80;
    // SHA256 input length is 131 * 8 = 1048, encoded big endian.
    message[190]=4; message[191]=24;
    uint h[8]={0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19};
    for (uint block=0;block<3;block++) {
        uint w[64];
        for (uint i=0;i<16;i++) {
            uint k=block*64+i*4;
            w[i]=((uint)message[k]<<24)|((uint)message[k+1]<<16)|((uint)message[k+2]<<8)|message[k+3];
        }
        for (uint i=16;i<64;i++) {
            uint x=w[i-15], y=w[i-2];
            w[i]=w[i-16]+(rr(x,7)^rr(x,18)^(x>>3))+w[i-7]+(rr(y,17)^rr(y,19)^(y>>10));
        }
        uint a=h[0],b=h[1],c=h[2],d=h[3],e=h[4],f=h[5],g=h[6],v=h[7];
        for (uint i=0;i<64;i++) {
            uint t1=v+(rr(e,6)^rr(e,11)^rr(e,25))+((e&f)^((~e)&g))+SHA_K[i]+w[i];
            uint t2=(rr(a,2)^rr(a,13)^rr(a,22))+((a&b)^(a&c)^(b&c));
            v=g;g=f;f=e;e=d+t1;d=c;c=b;b=a;a=t1+t2;
        }
        h[0]+=a;h[1]+=b;h[2]+=c;h[3]+=d;h[4]+=e;h[5]+=f;h[6]+=g;h[7]+=v;
    }
    ulong a[25];
    for (uint i=0;i<25;i++) a[i]=0;
    for (uint i=0;i<32;i++) {
        uchar byte=(uchar)(h[i/4]>>(24-8*(i%4)));
        a[i/8]|=(ulong)byte<<(8*(i%8));
    }
    // Keccak padding (0x01), deliberately different from SHA3 (0x06).
    a[4]^=1UL; a[16]^=0x8000000000000000UL;
    for (uint round=0;round<24;round++) {
        ulong c[5], d[5], b[25];
        for (uint x=0;x<5;x++) c[x]=a[x]^a[x+5]^a[x+10]^a[x+15]^a[x+20];
        for (uint x=0;x<5;x++) d[x]=c[(x+4)%5]^rl(c[(x+1)%5],1);
        for (uint y=0;y<5;y++) for (uint x=0;x<5;x++) {
            uint i=x+5*y;
            b[y+5*((2*x+3*y)%5)]=rl(a[i]^d[x],KECCAK_RHO[i]);
        }
        for (uint y=0;y<5;y++) for (uint x=0;x<5;x++)
            a[x+5*y]=b[x+5*y]^((~b[(x+1)%5+5*y])&b[(x+2)%5+5*y]);
        a[0]^=KECCAK_RC[round];
    }
    for (uint i=0;i<32;i++) seal[i]=(uchar)(a[i/8]>>(8*(i%8)));
}
__kernel void mine(__global const uchar *prefix, __global const uchar *target,
                   ulong start, uint attempts, __global uint *found, __global ulong *answer) {
    for (ulong offset=get_global_id(0);offset<(ulong)attempts;offset+=get_global_size(0)) {
        if (atomic_add((volatile __global uint *)found,0U)) return;
        ulong nonce=start+offset;
        uchar seal[32]; make_seal(prefix,nonce,seal);
        int cmp=0;
        for (int i=31;i>=0;i--) {
            if (seal[i]!=target[i]) { cmp=seal[i]<target[i] ? -1 : 1; break; }
        }
        if (cmp<=0 && atomic_cmpxchg((volatile __global uint *)found,0U,1U)==0U) {
            answer[0]=nonce;
            return;
        }
    }
}
// Hardware tests can compare exact digests, rather than only valid nonces.
__kernel void seal_vector(__global const uchar *prefix, ulong nonce, __global uchar *out) {
    uchar seal[32]; make_seal(prefix,nonce,seal);
    for (uint i=0;i<32;i++) out[i]=seal[i];
}
