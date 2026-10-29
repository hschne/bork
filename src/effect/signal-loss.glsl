// Final catch / catch / catch / tear / blackout. Both adapters use this body.
uint lossHash(uint n) {
    n ^= n >> 16u;
    n *= 0x7feb352du;
    n ^= n >> 15u;
    n *= 0x846ca68bu;
    return n ^ (n >> 16u);
}
float lossNoise(uint n) { return float(lossHash(n) >> 8u) / 16777216.0; }

vec4 lossSample(vec2 p) {
    if (p.x < 0.0 || p.x > 1.0) return vec4(0.027, 0.035, 0.031, 1.0);
    return sampleDesktop(p);
}

vec4 bork(vec2 uv, float seconds, uint output_seed) {
    float t = max(0.0, seconds);
    // Remain black if the guardian is briefly late handing off to the card.
    if (t >= 0.98) return vec4(0.027, 0.035, 0.031, 1.0);
    float sample_time = t;
    if (t >= HOLD_A.x && t < HOLD_A.y) sample_time = HOLD_A.x;
    if (t >= HOLD_B.x && t < HOLD_B.y) sample_time = HOLD_B.x;
    if (t >= HOLD_C.x && t < HOLD_C.y) sample_time = HOLD_C.x;
    uint seed = LOSS_SEED ^ lossHash(output_seed);
    uint tick = uint(floor(sample_time / 0.045));
    float bands = 10.0 + floor(lossNoise(seed ^ 19u) * 5.0);
    uint band = uint(floor(uv.y * bands));
    float shift = (lossNoise(tick ^ band * 13u ^ seed) - 0.5) * 0.025;
    if (band == tick % uint(bands)) shift += (lossNoise(seed ^ 53u) - 0.5) * 0.16;
    if (t >= 0.70) {
        shift += (lossNoise(band * 8u ^ uint(floor(t / 0.055)) ^ seed) - 0.5) * 0.54;
        float collapse = (t - 0.70) / 0.28;
        if (uv.y < collapse * 0.46 || uv.y > 1.0 - collapse * 0.46)
            return vec4(0.027, 0.035, 0.031, 1.0);
    }
    vec2 p = uv + vec2(shift, 0.0);
    vec3 colour = lossSample(p).rgb;
    float ghost_shift = (lossNoise(seed ^ 71u) - 0.5) * 0.04;
    colour = mix(colour, lossSample(uv + vec2(ghost_shift, 0.0)).rgb, 0.19);
    float separation = 0.0008 + lossNoise(tick ^ seed ^ 81u) * 0.002;
    colour.r = mix(colour.r, lossSample(p + vec2(separation, 0.0)).r, 0.5);
    colour.b = mix(colour.b, lossSample(p - vec2(separation, 0.0)).b, 0.5);
    float tracking_y = fract(float(tick) * 0.043 + lossNoise(seed ^ 97u));
    float tracking = step(tracking_y, uv.y) * step(uv.y, tracking_y + 0.07);
    colour *= 1.0 - tracking * 0.60;
    colour *= 1.0 - 0.15 * step(0.75, fract(uv.y * 180.0));
    return vec4(colour, 1.0);
}
