// Both the Hyprland adapter and preview compile this exact implementation.
// Only texture access, coordinates and shader language declarations differ.
uint hashBits(uint n) {
    n ^= n >> 16u;
    n *= 0x7feb352du;
    n ^= n >> 15u;
    n *= 0x846ca68bu;
    return n ^ (n >> 16u);
}
float noiseValue(uint n) { return float(hashBits(n) >> 8u) / 16777216.0; }

float burstAt(float seconds, uint seed) {
    // Fixed half-second candidate cells, not a repeating burst interval.
    // Randomly skipped cells, jittered starts and variable lengths produce
    // irregular gaps. Changing frequency adds events without rephasing time.
    float cell = floor(seconds * 2.0);
    float burst = 0.0;
    // Maximum length is 1.75 * 1.1 s: only these five cells can overlap now.
    for (int i = 0; i < 5; ++i) {
        float candidate = cell - float(i);
        if (candidate < 0.0) continue;
        uint event = hashBits(uint(candidate) ^ seed);
        if (noiseValue(event) >= FREQUENCY * 0.5) continue;
        float start = (candidate + noiseValue(event ^ 0x68bc21ebu)) * 0.5;
        float length = DURATION * (0.25 + 1.5 * noiseValue(event ^ 0x02e5be93u));
        float age = seconds - start;
        float edge = min(0.025, length * 0.2);
        float envelope = smoothstep(0.0, edge, age)
            * (1.0 - smoothstep(length - edge, length, age));
        burst = max(burst, envelope);
    }
    return burst;
}

vec4 signalLoss(vec2 uv, float seconds, uint seed) {
    seconds = max(seconds, 0.0);
    seed = hashBits(seed);
    float tearsBurst = burstAt(seconds, seed ^ 0xa511e9b3u);
    float bandsBurst = burstAt(seconds, seed ^ 0x63d83595u);
    float colourBurst = burstAt(seconds, seed ^ 0xb5297a4du);
    float noiseBurst = burstAt(seconds, seed ^ 0x1b56c4e9u);
    float ghostBurst = burstAt(seconds, seed ^ 0x9e3779b9u);
    uint line = uint(floor(uv.y * 240.0));
    uint band = uint(floor(uv.y * 18.0));
    uint tearFrame = uint(floor(seconds * 11.0)) * 0x85ebca6bu;
    uint bandFrame = uint(floor(seconds * 7.0)) * 0xc2b2ae35u;
    float tear = step(0.97, noiseValue(line ^ tearFrame ^ seed));
    float wide = step(0.72, noiseValue(band ^ bandFrame ^ seed));
    float shift = tearsBurst * (noiseValue(line ^ tearFrame ^ seed ^ 17u) - 0.5) * tear * TEARS * 0.065
        + bandsBurst * (noiseValue(band ^ bandFrame ^ seed ^ 31u) - 0.5) * wide * BANDS * 0.35;
    vec2 p = uv + vec2(shift, 0.0);
    float separation = COLOUR * (0.001 + colourBurst * 0.014);
    vec4 c = sampleDesktop(p);
    c.r = sampleDesktop(p + vec2(separation, 0.0)).r;
    c.b = sampleDesktop(p - vec2(separation, 0.0)).b;
    float ghostShift = (noiseValue(seed ^ uint(floor(seconds * 5.0)) ^ 47u) - 0.5) * 0.07;
    vec3 ghost = sampleDesktop(p + vec2(ghostShift, 0.0)).rgb;
    c.rgb = mix(c.rgb, ghost, GHOST * ghostBurst * 0.45);
    float grain = noiseValue(uint(floor(uv.x * 1280.0)) + line * 1280u
        ^ uint(floor(seconds * 24.0)) * 0x27d4eb2du ^ seed);
    c.rgb += (grain - 0.5) * NOISE * noiseBurst * 0.45;
    c.rgb *= 1.0 - NOISE * 0.15 * step(0.5, fract(uv.y * 360.0));
    return vec4(clamp(c.rgb, 0.0, 1.0), c.a);
}
