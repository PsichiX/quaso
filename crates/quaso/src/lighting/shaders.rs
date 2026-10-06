pub fn gbuffer_fragment() -> String {
    GBUFFER_FRAGMENT
        .replace("VERTEX_EMISSION_FUNCTIONS", VERTEX_EMISSION_FUNCTIONS)
        .replace(
            "ALBEDO",
            "((u_maps & 4) != 0 ? vec4(1.0) : texture(u_image, v_uv)) * albedo_tint()",
        )
}

pub fn composite_fragment(shade: Option<&str>) -> String {
    with_shade(&with_shadow_mask(COMPOSITE_FRAGMENT), shade)
}

pub fn forward_fragment(shade: Option<&str>) -> String {
    with_shade(
        &FORWARD_FRAGMENT.replace("VERTEX_EMISSION_FUNCTIONS", VERTEX_EMISSION_FUNCTIONS),
        shade,
    )
}

pub fn tonemap_fragment(grade: Option<&str>) -> String {
    TONEMAP_FRAGMENT.replace("GRADE_FUNCTION", grade.unwrap_or(DEFAULT_GRADE))
}

pub fn pass_fragment(pass: &str) -> String {
    with_shadow_mask(&PASS_FRAGMENT.replace("PASS_FUNCTION", pass))
}

fn with_shade(source: &str, shade: Option<&str>) -> String {
    source.replace(
        "SHADE_FUNCTION",
        &format!("{SURFACE_STRUCT}\n{}", shade.unwrap_or(DEFAULT_SHADE)),
    )
}

const SURFACE_STRUCT: &str = r#"struct Surface {
    vec3 albedo;
    vec3 normal;
    vec3 light;
    vec3 ambient;
    float shadow;
    vec2 scene_uv;
    vec2 screen_uv;
};"#;

const DEFAULT_SHADE: &str = r#"vec3 shade(Surface surface) {
    return surface.albedo * surface.light;
}"#;

const WATER_SURFACE_STRUCT: &str = r#"struct WaterSurface {
    vec3 bed;
    vec3 body;
    vec3 reflection;
    vec3 glint;
    vec3 halo;
    vec3 light;
    vec3 normal;
    float fresnel;
    float depth;
    float foam;
    float shadow;
    float shadow_dim;
    float shoreline;
    vec2 ripple_uv;
    vec2 scene_uv;
    vec2 screen_uv;
};"#;

const DEFAULT_WATER: &str = r#"vec4 water_color(WaterSurface water) {
    vec3 color = mix(water.body, water.reflection, water.fresnel)
        + water.glint * water.fresnel * 2.0
        + water.halo;
    color *= water.shadow_dim;
    color = mix(color, water.light, water.foam);
    return vec4(color, max(smoothstep(0.0, 0.1, water.depth), water.foam) * water.shoreline);
}"#;

const DEFAULT_GRADE: &str = r#"vec3 grade(vec3 color, vec2 screen_uv) {
    return aces(color);
}"#;

const PASS_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
out vec4 o_color;
uniform sampler2DArray u_source;
uniform sampler2DArray u_albedo;
uniform sampler2DArray u_emission;
uniform sampler2DArray u_absorption;
uniform sampler2DArray u_normal;
uniform sampler2DArray u_light_map;
uniform sampler2DArray u_shadow;
uniform vec2 u_target_size;
uniform vec2 u_scene_size;
uniform vec4 u_view_rect;
uniform float u_texel_world;
uniform float u_shadow_softness;
uniform vec3 u_shadow_light;
uniform vec3 u_shadow_open;

SHADOW_MASK_FUNCTION

vec4 source_color(vec2 screen_uv) {
    return texture(u_source, vec3(screen_uv, 0.0));
}

vec4 scene_albedo(vec2 scene_uv) {
    vec4 albedo = texture(u_albedo, vec3(scene_uv, 0.0));
    return albedo.a > 0.0 ? vec4(albedo.rgb / albedo.a, albedo.a) : vec4(0.0);
}

vec3 scene_normal(vec2 scene_uv) {
    vec4 normal = texture(u_normal, vec3(scene_uv, 0.0));
    if (normal.a <= 0.0) {
        return vec3(0.0, 0.0, 1.0);
    }
    vec2 normal_xy = normal.rg / normal.a * 2.0 - 1.0;
    return normalize(vec3(normal_xy, sqrt(max(1.0 - dot(normal_xy, normal_xy), 0.0))));
}

vec3 scene_emission(vec2 scene_uv) {
    return texture(u_emission, vec3(scene_uv, 0.0)).rgb;
}

float scene_opacity(vec2 scene_uv) {
    vec3 sigma = texture(u_absorption, vec3(scene_uv, 0.0)).rgb;
    return 1.0 - exp(-dot(sigma, vec3(1.0 / 3.0)) * u_texel_world);
}

vec2 scene_pixel() {
    return u_view_rect.zw / u_target_size;
}

vec3 scene_light(vec2 scene_uv) {
    return texture(u_light_map, vec3(scene_uv, 0.0)).rgb;
}

PASS_FUNCTION

void main() {
    vec2 screen_uv = gl_FragCoord.xy / u_target_size;
    vec2 scene_uv = u_view_rect.xy + screen_uv * u_view_rect.zw;
    o_color = pass_color(screen_uv, scene_uv);
}
"#;

pub fn water_fragment(water: Option<&str>) -> String {
    with_shadow_mask(WATER_FRAGMENT).replace(
        "WATER_FUNCTION",
        &format!(
            "{WATER_SURFACE_STRUCT}
{}",
            water.unwrap_or(DEFAULT_WATER)
        ),
    )
}

pub fn shadow_fragment() -> String {
    with_shadow_mask(SHADOW_FRAGMENT)
}

fn with_shadow_mask(source: &str) -> String {
    source
        .replace("SHADOW_MASK_FUNCTION", SHADOW_MASK)
        .replace("SHADOW_BLUR_PIXELS", "64.0")
}

/// With the vertex emission flag, the vertex color RGB goes to the emission and only the
/// vertex alpha reaches the albedo.
const VERTEX_EMISSION_FUNCTIONS: &str = r#"vec4 albedo_tint() {
    return (u_maps & 16) != 0 ? vec4(1.0, 1.0, 1.0, v_color.a) : v_color;
}

vec3 emission_tint() {
    return (u_maps & 16) != 0 ? v_color.rgb : vec3(1.0);
}"#;

pub fn gbuffer_text_fragment() -> String {
    GBUFFER_FRAGMENT
        .replace("VERTEX_EMISSION_FUNCTIONS", VERTEX_EMISSION_FUNCTIONS)
        .replace(
            "ALBEDO",
            "vec4(v_color.rgb, v_color.a * texture(u_image, v_uv).x)",
        )
}

const GBUFFER_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
in vec4 v_color;
in vec3 v_uv;
layout(location = 0) out vec4 o_albedo;
layout(location = 1) out vec4 o_emission;
layout(location = 2) out vec4 o_absorption;
layout(location = 3) out vec4 o_normal;
uniform sampler2DArray u_image;
uniform sampler2DArray u_normal_map;
uniform sampler2DArray u_emission_map;
uniform sampler2DArray u_absorption_map;
uniform int u_maps;
uniform vec3 u_emission;
uniform vec3 u_absorption;
uniform float u_surface_mode;

VERTEX_EMISSION_FUNCTIONS

vec3 rotate_normal(vec3 normal, vec2 uv_dx, vec2 uv_dy) {
    float determinant = uv_dx.x * uv_dy.y - uv_dy.x * uv_dx.y;
    if (abs(determinant) < 1.0e-12) {
        return normal;
    }
    float flip = sign(determinant);
    vec2 tangent = normalize(vec2(uv_dy.y, -uv_dx.y)) * flip;
    vec2 bitangent = normalize(vec2(-uv_dy.x, uv_dx.x)) * flip;
    return normalize(vec3(normal.x * tangent - normal.y * bitangent, normal.z));
}

void main() {
    vec2 uv_dx = dFdx(v_uv.xy);
    vec2 uv_dy = dFdy(v_uv.xy);
    vec4 albedo = ALBEDO;
    float coverage = albedo.a;
    if (coverage <= 0.0) {
        discard;
    }
    vec3 emission = u_emission * emission_tint();
    if ((u_maps & 2) != 0) {
        emission *= texture(u_emission_map, v_uv).rgb;
    }
    vec3 absorption = u_absorption;
    if ((u_maps & 8) != 0) {
        absorption *= texture(u_absorption_map, v_uv).rgb;
    }
    vec3 normal = vec3(0.0, 0.0, 1.0);
    if ((u_maps & 1) != 0) {
        normal = rotate_normal(texture(u_normal_map, v_uv).xyz * 2.0 - 1.0, uv_dx, uv_dy);
    }
    o_albedo = vec4(albedo.rgb * coverage, coverage);
    o_emission = vec4(emission * coverage, 0.0);
    o_absorption = vec4(absorption * coverage, 0.0);
    o_normal = vec4(vec3(normal.xy * 0.5 + 0.5, u_surface_mode) * coverage, coverage);
}
"#;

pub const CASCADE_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
out vec4 o_color;
uniform sampler2DArray u_opacity;
uniform sampler2DArray u_emission;
uniform sampler2DArray u_upper;
uniform sampler2DArray u_seeds;
uniform int u_cascade;
uniform int u_cascade_count;
uniform vec2 u_size;
uniform vec2 u_scene_size;
uniform float u_base_interval;
uniform float u_probe_spacing;
uniform float u_gbuffer_scale;
uniform vec3 u_sky;
uniform vec3 u_sun;
uniform vec2 u_sun_direction;
uniform float u_sun_sharpness;
uniform float u_light_height;

const float TAU = 6.283185307179586;
const float OPAQUE = 0.001;

vec3 scene(sampler2DArray layer, vec2 position, float lod) {
    vec2 uv = position / u_scene_size;
    if (any(lessThan(uv, vec2(0.0))) || any(greaterThan(uv, vec2(1.0)))) {
        return vec3(0.0);
    }
    return textureLod(layer, vec3(uv, 0.0), lod).rgb;
}

float scene_distance(vec2 position) {
    vec2 outside = max(max(-position, position - u_scene_size), vec2(0.0));
    if (any(greaterThan(outside, vec2(0.0)))) {
        return max(length(outside), 1.0);
    }
    ivec2 texel = clamp(ivec2(position), ivec2(0), ivec2(u_scene_size) - 1);
    vec4 seed = texelFetch(u_seeds, ivec3(texel, 0), 0);
    if (seed.x < 0.0) {
        return 1.0e20;
    }
    return distance(seed.xy, position);
}

const float SKY_ELEVATION = 1.0;

float luminance(vec3 color) {
    return dot(color, vec3(0.2126, 0.7152, 0.0722));
}

/// `elevation` sums `luminance * tan(elevation)` of the gathered light.
/// A light found at distance d from the probe is `u_light_height / d` steep.
void march(
    vec2 origin,
    vec2 from,
    vec2 to,
    float step_size,
    out vec3 radiance,
    out vec3 transmittance,
    out float elevation
) {
    vec2 delta = to - from;
    float length_total = length(delta);
    vec2 direction = delta / max(length_total, 1.0e-6);
    radiance = vec3(0.0);
    transmittance = vec3(1.0);
    elevation = 0.0;
    float lod = max(log2(step_size * u_gbuffer_scale), 0.0);
    float t = 0.0;
    while (t < length_total) {
        float skip = scene_distance(from + direction * t) - 1.0;
        if (skip >= 1.0) {
            t += skip;
            continue;
        }
        float step_length = min(step_size, length_total - t);
        vec2 position = from + direction * (t + step_length * 0.5);
        t += step_length;
        vec3 opacity = min(scene(u_opacity, position, lod), vec3(1.0));
        vec3 emission = scene(u_emission, position, lod);
        float mean_opacity = dot(opacity, vec3(1.0 / 3.0));
        float mean_step_opacity = 1.0 - pow(1.0 - mean_opacity, step_length);
        float emission_scale = mean_opacity > 1.0e-4
            ? mean_step_opacity / mean_opacity
            : step_length;
        vec3 gathered = transmittance * emission * emission_scale;
        radiance += gathered;
        elevation += luminance(gathered) * u_light_height / max(distance(position, origin), 1.0);
        transmittance *= pow(1.0 - opacity, vec3(step_length));
        if (max(transmittance.r, max(transmittance.g, transmittance.b)) < OPAQUE) {
            transmittance = vec3(0.0);
            break;
        }
    }
}

void main() {
    ivec2 texel = ivec2(gl_FragCoord.xy);
    int side = 1 << (u_cascade + 1);
    ivec2 tile_size = ivec2(u_size) / side;
    ivec2 tile = texel / tile_size;
    ivec2 probe = texel - tile * tile_size;
    int direction_count = side * side;
    int direction_index = tile.y * side + tile.x;
    float angle = (float(direction_index) + 0.5) * TAU / float(direction_count);
    vec2 direction = vec2(cos(angle), sin(angle));
    float spacing = u_probe_spacing * float(side / 2);
    vec2 origin = (vec2(probe) + 0.5) * spacing;
    float growth = pow(4.0, float(u_cascade));
    float start = u_base_interval * (growth - 1.0) / 3.0;
    float end = u_base_interval * (growth * 4.0 - 1.0) / 3.0;
    vec3 radiance;
    vec3 transmittance;
    float elevation;

    if (u_cascade + 1 >= u_cascade_count) {
        march(origin, origin + direction * start, origin + direction * end, spacing, radiance, transmittance, elevation);
        float sun_lobe = pow(max(dot(direction, u_sun_direction), 0.0), u_sun_sharpness);
        vec3 sky = transmittance * (u_sky + u_sun * sun_lobe);
        elevation += luminance(sky) * SKY_ELEVATION;
        radiance += sky;
        o_color = vec4(radiance, elevation / max(luminance(radiance), 1.0e-6));
        return;
    }

    int upper_side = side * 2;
    float upper_spacing = spacing * 2.0;
    ivec2 upper_tile_size = ivec2(u_size) / upper_side;
    vec2 upper_coords = origin / upper_spacing - 0.5;
    vec2 upper_base = floor(upper_coords);
    vec2 weights = upper_coords - upper_base;
    vec3 result = vec3(0.0);
    float result_elevation = 0.0;
    for (int n = 0; n < 4; ++n) {
        ivec2 offset = ivec2(n % 2, n / 2);
        ivec2 upper_probe = clamp(ivec2(upper_base) + offset, ivec2(0), upper_tile_size - 1);
        vec2 upper_origin = (vec2(upper_probe) + 0.5) * upper_spacing;
        march(origin, origin + direction * start, upper_origin + direction * end, spacing, radiance, transmittance, elevation);
        vec3 upper = vec3(0.0);
        float upper_elevation = 0.0;
        for (int k = 0; k < 4; ++k) {
            int child = direction_index * 4 + k;
            ivec2 upper_tile = ivec2(child % upper_side, child / upper_side);
            vec4 sample_value = texelFetch(u_upper, ivec3(upper_tile * upper_tile_size + upper_probe, 0), 0);
            upper += sample_value.rgb;
            upper_elevation += luminance(sample_value.rgb) * sample_value.a;
        }
        upper_elevation /= max(luminance(upper), 1.0e-6);
        vec3 far = transmittance * upper * 0.25;
        vec2 weight = mix(1.0 - weights, weights, vec2(offset));
        result += weight.x * weight.y * (radiance + far);
        result_elevation += weight.x * weight.y * (elevation + luminance(far) * upper_elevation);
    }
    o_color = vec4(result, result_elevation / max(luminance(result), 1.0e-6));
}
"#;

pub const MEDIUM_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
layout(location = 0) out vec4 o_opacity;
layout(location = 1) out vec4 o_emission;
uniform sampler2DArray u_emission;
uniform sampler2DArray u_absorption;
uniform float u_texel_world;

void main() {
    ivec3 texel = ivec3(gl_FragCoord.xy, 0);
    vec3 sigma = texelFetch(u_absorption, texel, 0).rgb;
    vec3 emission = texelFetch(u_emission, texel, 0).rgb;
    vec3 opacity = 1.0 - exp(-sigma * u_texel_world);
    o_opacity = vec4(opacity, 1.0);
    o_emission = vec4(emission * dot(opacity, vec3(1.0 / 3.0)), 1.0);
}
"#;

pub const SEED_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
out vec4 o_color;
uniform sampler2DArray u_emission;
uniform sampler2DArray u_absorption;
uniform vec2 u_scene_size;
uniform int u_invert;

void main() {
    vec3 uv = vec3(gl_FragCoord.xy / u_scene_size, 0.0);
    vec3 emission = texture(u_emission, uv).rgb;
    vec3 absorption = texture(u_absorption, uv).rgb;
    vec3 occupancy = max(emission, absorption);
    bool occupied = max(occupancy.r, max(occupancy.g, occupancy.b)) > 1.0e-4;
    if (occupied != (u_invert != 0)) {
        o_color = vec4(gl_FragCoord.xy, 0.0, 1.0);
    } else {
        o_color = vec4(-1.0);
    }
}
"#;

pub const JUMP_FLOOD_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
out vec4 o_color;
uniform sampler2DArray u_seeds;
uniform int u_step;
uniform vec2 u_scene_size;

void main() {
    ivec2 texel = ivec2(gl_FragCoord.xy);
    ivec2 size = ivec2(u_scene_size);
    vec4 best = vec4(-1.0);
    float best_distance = 1.0e20;
    for (int y = -1; y <= 1; ++y) {
        for (int x = -1; x <= 1; ++x) {
            ivec2 sample_texel = texel + ivec2(x, y) * u_step;
            if (any(lessThan(sample_texel, ivec2(0))) || any(greaterThanEqual(sample_texel, size))) {
                continue;
            }
            vec4 seed = texelFetch(u_seeds, ivec3(sample_texel, 0), 0);
            if (seed.x < 0.0) {
                continue;
            }
            float seed_distance = distance(seed.xy, gl_FragCoord.xy);
            if (seed_distance < best_distance) {
                best_distance = seed_distance;
                best = seed;
            }
        }
    }
    o_color = best;
}
"#;

pub const LIGHT_MAP_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
out vec4 o_color;
uniform sampler2DArray u_cascade;
uniform sampler2DArray u_absorption;
uniform sampler2DArray u_surface;
uniform vec2 u_size;
uniform vec2 u_scene_size;
uniform float u_probe_spacing;
uniform float u_texel_world;

vec3 light_at(vec2 scene_position) {
    ivec2 tile_size = ivec2(u_size) / 2;
    vec2 texel = clamp(scene_position / u_probe_spacing, vec2(0.5), vec2(tile_size) - 0.5);
    vec3 sum = vec3(0.0);
    for (int k = 0; k < 4; ++k) {
        ivec2 tile = ivec2(k % 2, k / 2);
        vec2 position = vec2(tile * tile_size) + texel;
        sum += texture(u_cascade, vec3(position / u_size, 0.0)).rgb;
    }
    return sum * 0.25;
}

/// Inside an opaque object the light is the light just outside its nearest edge.
/// Forward sprites drawn over the object then match its surroundings, not a black hole.
void main() {
    vec2 scene_position = gl_FragCoord.xy;
    vec3 light = light_at(scene_position);
    vec3 sigma = texture(u_absorption, vec3(scene_position / u_scene_size, 0.0)).rgb;
    float opacity = 1.0 - exp(-dot(sigma, vec3(1.0 / 3.0)) * u_texel_world);
    ivec2 scene_texel = clamp(ivec2(scene_position), ivec2(0), ivec2(u_scene_size) - 1);
    vec4 surface = texelFetch(u_surface, ivec3(scene_texel, 0), 0);
    if (opacity > 0.0 && surface.x >= 0.0) {
        vec2 outward = surface.xy - scene_position;
        float outward_length = length(outward);
        vec2 edge = outward_length > 0.0 ? surface.xy + outward / outward_length : surface.xy;
        light = mix(light, light_at(edge), opacity);
    }
    o_color = vec4(light, 1.0);
}
"#;

pub const COMPOSITE_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
out vec4 o_color;
uniform sampler2DArray u_albedo;
uniform sampler2DArray u_emission;
uniform sampler2DArray u_absorption;
uniform sampler2DArray u_normal;
uniform sampler2DArray u_light_map;
uniform sampler2DArray u_cascade;
uniform sampler2DArray u_surface;
uniform sampler2DArray u_shadow;
uniform sampler2DArray u_seeds;
uniform vec2 u_target_size;
uniform vec2 u_scene_size;
uniform vec2 u_cascade_size;
uniform vec3 u_shadow_light;
uniform vec3 u_shadow_open;
uniform float u_shadow_softness;
uniform float u_texel_world;
uniform vec3 u_ambient;
uniform float u_probe_spacing;
uniform vec4 u_view_rect;
uniform int u_debug;
uniform vec4 u_tile;

SHADOW_MASK_FUNCTION

SHADE_FUNCTION

const float TAU = 6.283185307179586;
const float TILE_BORDER = 2.0;

const float MIN_STEEPNESS = 0.25;
const float MAX_STEEPNESS = 20.0;

vec3 directional_light(vec2 scene_position, vec3 normal) {
    ivec2 tile_size = ivec2(u_cascade_size) / 2;
    vec2 texel = clamp(scene_position / u_probe_spacing, vec2(0.5), vec2(tile_size) - 0.5);
    vec3 sum = vec3(0.0);
    for (int k = 0; k < 4; ++k) {
        ivec2 tile = ivec2(k % 2, k / 2);
        float angle = (float(k) + 0.5) * TAU / 4.0;
        vec2 position = vec2(tile * tile_size) + texel;
        vec4 light = texture(u_cascade, vec3(position / u_cascade_size, 0.0));
        float steepness = clamp(light.a, MIN_STEEPNESS, MAX_STEEPNESS);
        vec3 light_direction = normalize(vec3(cos(angle), sin(angle), steepness));
        float weight = max(dot(normal, light_direction), 0.0) / light_direction.z;
        sum += light.rgb * weight;
    }
    return sum * 0.25;
}

const int SMOOTH_TAPS = 16;
const int BOUNDARY_STEPS = 8;

vec4 surface_at(vec2 scene_position) {
    ivec2 scene_texel = clamp(ivec2(scene_position), ivec2(0), ivec2(u_scene_size) - 1);
    return texelFetch(u_surface, ivec3(scene_texel, 0), 0);
}

vec3 surface_light_at(vec2 scene_position, vec3 normal) {
    vec4 surface = surface_at(scene_position);
    if (surface.x < 0.0) {
        return directional_light(scene_position, normal);
    }
    vec2 outward = surface.xy - scene_position;
    float outward_length = length(outward);
    vec2 surface_position = outward_length > 0.0
        ? surface.xy + outward / outward_length
        : surface.xy;
    return directional_light(surface_position, normal);
}

float luminance(vec3 color) {
    return dot(color, vec3(0.2126, 0.7152, 0.0722));
}

/// Steps from the pixel along the direction to the edge of the object, with the distance
/// to the nearest empty texel as a safe step, and reads the light just past that edge.
/// One tap stands for a whole sector of directions. A point read makes a bright light at
/// the edge into a stripe for each tap, so the tap reads the light map blurred to the
/// sector width at that distance. The normal shading is the ratio of the sharp reads.
vec3 boundary_light(vec2 scene_position, vec2 direction, vec3 normal) {
    vec2 position = scene_position;
    for (int step = 0; step < BOUNDARY_STEPS; ++step) {
        vec4 surface = surface_at(position);
        float edge_distance = surface.x >= 0.0 ? distance(surface.xy, position) : 0.0;
        if (edge_distance < 1.0) {
            break;
        }
        position += direction * edge_distance;
    }
    position += direction;
    float sector_width = distance(position, scene_position) * TAU / float(SMOOTH_TAPS);
    float lod = log2(max(sector_width, 1.0));
    vec3 blurred = textureLod(u_light_map, vec3(position / u_scene_size, 0.0), lod).rgb;
    float flat_light = luminance(directional_light(position, vec3(0.0, 0.0, 1.0)));
    float shaded_light = luminance(directional_light(position, normal));
    float shade = flat_light > 1.0e-6 ? clamp(shaded_light / flat_light, 0.0, 4.0) : 1.0;
    return blurred * shade;
}

/// An overview tile covers the whole target and keeps only the pixels in `u_tile`.
/// The tile then shows the whole view, scaled down.
void main() {
    vec2 view_uv = gl_FragCoord.xy / u_target_size;
    bool tile = u_tile.z > 0.0;
    if (tile) {
        vec2 pixel = gl_FragCoord.xy - u_tile.xy;
        if (any(lessThan(pixel, vec2(0.0))) || any(greaterThanEqual(pixel, u_tile.zw))) {
            discard;
        }
        if (any(lessThan(pixel, vec2(TILE_BORDER))) || any(greaterThanEqual(pixel, u_tile.zw - TILE_BORDER))) {
            o_color = vec4(0.0, 0.0, 0.0, 1.0);
            return;
        }
        view_uv = pixel / u_tile.zw;
    }
    vec2 scene_uv = u_view_rect.xy + view_uv * u_view_rect.zw;
    vec3 uv = vec3(scene_uv, 0.0);
    vec4 albedo = texture(u_albedo, uv);
    vec3 emission = texture(u_emission, uv).rgb;
    vec4 normal_sample = texture(u_normal, uv);
    vec3 normal = vec3(0.0, 0.0, 1.0);
    float surface_mode = 0.0;
    if (normal_sample.a > 0.0) {
        vec2 normal_xy = normal_sample.rg / normal_sample.a * 2.0 - 1.0;
        normal = normalize(vec3(normal_xy, sqrt(max(1.0 - dot(normal_xy, normal_xy), 0.0))));
        surface_mode = normal_sample.b / normal_sample.a;
    }
    vec2 scene_position = scene_uv * u_scene_size;
    vec3 light = directional_light(scene_position, normal);
    vec3 sigma = texture(u_absorption, uv).rgb;
    float opacity = 1.0 - exp(-dot(sigma, vec3(1.0 / 3.0)) * u_texel_world);
    vec4 surface = surface_at(scene_position);
    if (opacity > 0.0 && surface.x >= 0.0) {
        vec3 surface_light = vec3(0.0);
        if ((int(floor(surface_mode * 4.0 + 0.5)) & 2) != 0) {
            for (int k = 0; k < SMOOTH_TAPS; ++k) {
                float angle = (float(k) + 0.5) * TAU / float(SMOOTH_TAPS);
                surface_light += boundary_light(scene_position, vec2(cos(angle), sin(angle)), normal);
            }
            surface_light /= float(SMOOTH_TAPS);
        } else {
            surface_light = surface_light_at(scene_position, normal);
        }
        light = mix(light, surface_light, opacity);
    }
    float shadow = shadow_mask(scene_uv);
    light = shadowed(light, shadow);
    light += u_ambient;
    if (u_debug == 1) {
        o_color = vec4(albedo.rgb, 1.0);
    } else if (u_debug == 2) {
        o_color = vec4(emission, 1.0);
    } else if (u_debug == 3) {
        vec3 sigma = texture(u_absorption, uv).rgb;
        o_color = vec4(sigma / (sigma + 1.0), 1.0);
    } else if (u_debug == 4) {
        o_color = vec4(normal * 0.5 + 0.5, 1.0);
    } else if (u_debug == 5) {
        o_color = vec4(texture(u_light_map, uv).rgb, 1.0);
    } else if (u_debug == 6) {
        o_color = vec4(vec3(1.0 - shadow), 1.0);
    } else if (u_debug == 7) {
        o_color = vec4(tile ? light / (light + 1.0) : light, 1.0);
    } else if (u_debug == 8) {
        ivec2 texel = clamp(ivec2(scene_position), ivec2(0), ivec2(u_scene_size) - 1);
        vec4 seed = texelFetch(u_seeds, ivec3(texel, 0), 0);
        float occluder_distance = seed.x < 0.0 ? 1.0e6 : distance(seed.xy, scene_position);
        float band = fract(occluder_distance / 8.0) < 0.5 ? 1.0 : 0.85;
        o_color = vec4(vec3(1.0 - exp(-occluder_distance / 32.0)) * band, 1.0);
    } else if (u_debug == 9) {
        o_color = vec4(albedo.rgb * 0.25, 1.0);
    } else {
        vec3 straight = albedo.a > 0.0 ? albedo.rgb / albedo.a : vec3(0.0);
        Surface surface = Surface(straight, normal, light, u_ambient, shadow, scene_uv, view_uv);
        o_color = vec4(shade(surface) * albedo.a + emission, albedo.a);
    }
}
"#;

pub const TONEMAP_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
out vec4 o_color;
uniform sampler2DArray u_hdr;
uniform vec2 u_target_size;
uniform sampler2DArray u_adapted;
uniform float u_exposure;
uniform int u_adaptation;
uniform float u_adaptation_key;
uniform vec2 u_adaptation_range;
uniform int u_tonemap;
uniform sampler2DArray u_bloom;
uniform float u_bloom_strength;
uniform int u_bloom_ready;
uniform int u_bloom_view;
uniform vec4 u_tile;

const float TILE_BORDER = 2.0;

vec3 aces(vec3 color) {
    return clamp(
        (color * (2.51 * color + 0.03)) / (color * (2.43 * color + 0.59) + 0.14),
        0.0,
        1.0
    );
}

GRADE_FUNCTION

void main() {
    vec2 view_uv = gl_FragCoord.xy / u_target_size;
    bool tile = u_tile.z > 0.0;
    if (tile) {
        vec2 pixel = gl_FragCoord.xy - u_tile.xy;
        if (any(lessThan(pixel, vec2(0.0))) || any(greaterThanEqual(pixel, u_tile.zw))) {
            discard;
        }
        if (any(lessThan(pixel, vec2(TILE_BORDER))) || any(greaterThanEqual(pixel, u_tile.zw - TILE_BORDER))) {
            o_color = vec4(0.0, 0.0, 0.0, 1.0);
            return;
        }
        view_uv = pixel / u_tile.zw;
    }
    vec4 hdr = texture(u_hdr, vec3(view_uv, 0.0));
    vec3 bloom = u_bloom_ready != 0 ? texture(u_bloom, vec3(view_uv, 0.0)).rgb : vec3(0.0);
    float exposure = u_exposure;
    if (u_adaptation != 0) {
        float average = exp(texelFetch(u_adapted, ivec3(0), 0).r);
        exposure *= clamp(u_adaptation_key / average, u_adaptation_range.x, u_adaptation_range.y);
    }
    vec3 color;
    float alpha = clamp(hdr.a, 0.0, 1.0);
    if (u_bloom_view != 0) {
        color = aces(bloom * exposure);
        alpha = 1.0;
    } else if (u_tonemap != 0) {
        color = clamp(grade(mix(hdr.rgb, bloom, u_bloom_strength) * exposure, view_uv), 0.0, 1.0);
    } else {
        color = clamp(hdr.rgb, 0.0, 1.0);
    }
    o_color = vec4(color, alpha);
}
"#;

/// The downsample is the 13 tap filter of Jimenez, "Next Generation Post Processing in Call
/// of Duty: Advanced Warfare". Five overlapped boxes give a round blur that does not shimmer
/// when the camera moves, which a plain 2x2 mipmap average does.
pub const BLOOM_DOWN_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
out vec4 o_color;
uniform sampler2DArray u_source;
uniform vec2 u_source_size;
uniform vec2 u_target_size;
uniform int u_first;
uniform float u_input_scale;
uniform float u_output_scale;

const float MAX_VALUE = 60000.0;

vec3 tap(vec2 uv) {
    return clamp(texture(u_source, vec3(uv, 0.0)).rgb * u_input_scale, vec3(0.0), vec3(MAX_VALUE));
}

float karis_weight(vec3 color) {
    return 1.0 / (1.0 + dot(color, vec3(0.2126, 0.7152, 0.0722)));
}

void main() {
    vec2 uv = gl_FragCoord.xy / u_target_size;
    vec2 texel = 1.0 / u_source_size;
    vec3 a = tap(uv + texel * vec2(-2.0, 2.0));
    vec3 b = tap(uv + texel * vec2(0.0, 2.0));
    vec3 c = tap(uv + texel * vec2(2.0, 2.0));
    vec3 d = tap(uv + texel * vec2(-2.0, 0.0));
    vec3 e = tap(uv);
    vec3 f = tap(uv + texel * vec2(2.0, 0.0));
    vec3 g = tap(uv + texel * vec2(-2.0, -2.0));
    vec3 h = tap(uv + texel * vec2(0.0, -2.0));
    vec3 i = tap(uv + texel * vec2(2.0, -2.0));
    vec3 j = tap(uv + texel * vec2(-1.0, 1.0));
    vec3 k = tap(uv + texel * vec2(1.0, 1.0));
    vec3 l = tap(uv + texel * vec2(-1.0, -1.0));
    vec3 m = tap(uv + texel * vec2(1.0, -1.0));
    vec3 groups[5] = vec3[5](
        (j + k + l + m) * 0.25,
        (a + b + d + e) * 0.25,
        (b + c + e + f) * 0.25,
        (d + e + g + h) * 0.25,
        (e + f + h + i) * 0.25
    );
    float weights[5] = float[5](0.5, 0.125, 0.125, 0.125, 0.125);
    vec3 sum = vec3(0.0);
    float total = 0.0;
    for (int index = 0; index < 5; ++index) {
        float weight = weights[index] * (u_first != 0 ? karis_weight(groups[index]) : 1.0);
        sum += groups[index] * weight;
        total += weight;
    }
    o_color = vec4(sum / total * u_output_scale, 1.0);
}
"#;

/// The upsample reads the smaller level with a 3x3 tent filter and adds it to the larger level.
pub const BLOOM_UP_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
out vec4 o_color;
uniform sampler2DArray u_source;
uniform vec2 u_source_size;
uniform vec2 u_target_size;
uniform float u_scale;

vec3 tap(vec2 uv) {
    return texture(u_source, vec3(uv, 0.0)).rgb;
}

void main() {
    vec2 uv = gl_FragCoord.xy / u_target_size;
    vec2 texel = 1.0 / u_source_size;
    vec3 sum = tap(uv) * 4.0;
    sum += (tap(uv + texel * vec2(-1.0, 0.0)) + tap(uv + texel * vec2(1.0, 0.0))
        + tap(uv + texel * vec2(0.0, -1.0)) + tap(uv + texel * vec2(0.0, 1.0))) * 2.0;
    sum += tap(uv + texel * vec2(-1.0, -1.0)) + tap(uv + texel * vec2(1.0, -1.0))
        + tap(uv + texel * vec2(-1.0, 1.0)) + tap(uv + texel * vec2(1.0, 1.0));
    o_color = vec4(sum / 16.0 * u_scale, 1.0);
}
"#;

pub const LUMINANCE_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
out vec4 o_color;
uniform sampler2DArray u_hdr;
uniform vec2 u_size;
uniform vec2 u_screen_size;
uniform vec3 u_focus;

void main() {
    vec2 uv = gl_FragCoord.xy / u_size;
    vec3 color = texture(u_hdr, vec3(uv, 0.0)).rgb;
    float luminance = dot(color, vec3(0.2126, 0.7152, 0.0722));
    float weight = 1.0;
    if (u_focus.z > 0.0) {
        vec2 offset = (uv * u_screen_size - u_focus.xy) / u_focus.z;
        weight = exp(-0.5 * dot(offset, offset));
    }
    o_color = vec4(log(max(luminance, 1.0e-4)) * weight, weight, 0.0, 1.0);
}
"#;

pub const ADAPTATION_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
out vec4 o_color;
uniform sampler2DArray u_luminance;
uniform sampler2DArray u_previous;
uniform float u_lod;
uniform float u_blend;

void main() {
    vec2 weighted = textureLod(u_luminance, vec3(0.5, 0.5, 0.0), u_lod).rg;
    float current = weighted.x / max(weighted.y, 1.0e-6);
    float previous = texelFetch(u_previous, ivec3(0), 0).r;
    o_color = vec4(mix(previous, current, u_blend), 0.0, 0.0, 1.0);
}
"#;

pub const FORWARD_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
in vec4 v_color;
in vec3 v_uv;
out vec4 o_color;
uniform sampler2DArray u_image;
uniform sampler2DArray u_emission_map;
uniform sampler2DArray u_light_map;
uniform int u_maps;
uniform vec3 u_emission;
uniform vec2 u_target_size;
uniform vec4 u_view_rect;
uniform vec3 u_ambient;
uniform int u_debug;

VERTEX_EMISSION_FUNCTIONS
SHADE_FUNCTION

void main() {
    if (u_debug != 0) {
        discard;
    }
    vec4 albedo = ((u_maps & 4) != 0 ? vec4(1.0) : texture(u_image, v_uv)) * albedo_tint();
    vec3 emission = u_emission * emission_tint();
    if ((u_maps & 2) != 0) {
        emission *= texture(u_emission_map, v_uv).rgb;
    }
    vec2 scene_uv = u_view_rect.xy + gl_FragCoord.xy / u_target_size * u_view_rect.zw;
    vec3 light = texture(u_light_map, vec3(scene_uv, 0.0)).rgb + u_ambient;
    vec2 screen_uv = gl_FragCoord.xy / u_target_size;
    Surface surface = Surface(albedo.rgb, vec3(0.0, 0.0, 1.0), light, u_ambient, 0.0, scene_uv, screen_uv);
    o_color = vec4(shade(surface) + emission, albedo.a);
}
"#;

pub const SHADOW_VERTEX: &str = r#"#version 300 es
layout(location = 0) in vec2 a_position;
layout(location = 1) in vec3 a_uv;
layout(location = 2) in vec4 a_color;
out vec4 v_color;
out vec3 v_uv;
out vec2 v_world;
uniform mat4 u_projection_view;
uniform vec2 u_shadow_offset;
uniform float u_height;
uniform int u_vertex_height;

void main() {
    float height = u_vertex_height != 0 ? u_height * a_color.r : u_height;
    v_world = a_position + u_shadow_offset * height;
    gl_Position = u_projection_view * vec4(v_world, 0.0, 1.0);
    v_color = a_color;
    v_uv = a_uv;
}
"#;

pub const SHADOW_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
in vec4 v_color;
in vec3 v_uv;
in vec2 v_world;
out vec4 o_color;
uniform sampler2DArray u_image;
uniform int u_maps;
uniform float u_opacity;
uniform float u_softness;
uniform int u_erase;

void main() {
    float alpha = (u_maps & 4) != 0 ? v_color.a : texture(u_image, v_uv).a * v_color.a;
    float coverage = clamp(alpha * u_opacity, 0.0, 1.0);
    if (u_erase != 0) {
        o_color = vec4(1.0 - coverage);
        return;
    }
    float world_per_pixel = max(length(dFdx(v_world)), 1.0e-4);
    float blur = clamp(u_softness / world_per_pixel / SHADOW_BLUR_PIXELS, 0.0, 1.0);
    o_color = vec4(1.0, blur, 0.0, coverage);
}
"#;

/// The shadow mask keeps the coverage in red and the blur radius, premultiplied by coverage, in green.
/// A wide mipmap sample gives the mean blur radius of the casters near the pixel.
/// That radius then sizes a spiral of taps, so each caster gets its own edge softness.
/// The radiance cascades already remove the sun behind opaque objects. A height shadow therefore
/// removes the blocked share of the light that is present, so the two shadows do not add up to black.
/// The cap keeps the light of local lights above the share of the open field light.
const SHADOW_MASK: &str = r#"const int SHADOW_TAPS = 12;

float shadow_mask(vec2 uv) {
    vec2 wide = textureLod(u_shadow, vec3(uv, 0.0), 5.0).rg;
    float radius = wide.g / max(wide.r, 1.0e-3) * SHADOW_BLUR_PIXELS + u_shadow_softness;
    float lod = log2(max(radius / 3.0, 1.0));
    vec2 texel = 1.0 / u_scene_size;
    float sum = textureLod(u_shadow, vec3(uv, 0.0), lod).r;
    for (int index = 0; index < SHADOW_TAPS; ++index) {
        float along = sqrt((float(index) + 0.5) / float(SHADOW_TAPS));
        float angle = float(index) * 2.39996;
        vec2 offset = vec2(cos(angle), sin(angle)) * along * radius * texel;
        sum += textureLod(u_shadow, vec3(uv + offset, 0.0), lod).r;
    }
    return sum / float(SHADOW_TAPS + 1);
}

vec3 shadowed(vec3 light, float shadow) {
    vec3 blocked = u_shadow_light * shadow;
    return light - min(blocked, light * blocked / max(u_shadow_open, vec3(1.0e-4)));
}"#;

/// The scene grid moves in whole steps of the top cascade spacing, so a probe always sits on
/// the same world point. The previous nearest cascade then needs only a whole probe shift.
/// A probe that comes into view has no history and takes the current value.
pub const TEMPORAL_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
out vec4 o_color;
uniform sampler2DArray u_current;
uniform sampler2DArray u_previous;
uniform vec2 u_size;
uniform vec2 u_shift;
uniform float u_history;

void main() {
    ivec2 texel = ivec2(gl_FragCoord.xy);
    ivec2 tile_size = ivec2(u_size) / 2;
    ivec2 tile_origin = texel / tile_size * tile_size;
    ivec2 previous = texel + ivec2(u_shift);
    ivec2 local = previous - tile_origin;
    vec4 current = texelFetch(u_current, ivec3(texel, 0), 0);
    if (any(lessThan(local, ivec2(0))) || any(greaterThanEqual(local, tile_size))) {
        o_color = current;
        return;
    }
    o_color = mix(current, texelFetch(u_previous, ivec3(previous, 0), 0), u_history);
}
"#;

pub const COPY_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
out vec4 o_color;
uniform sampler2DArray u_source;
uniform vec2 u_target_size;

void main() {
    o_color = texture(u_source, vec3(gl_FragCoord.xy / u_target_size, 0.0));
}
"#;

/// Water draws after the composite. It refracts the lit scene copy under it and mirrors
/// the bank around the shore line along the reflection direction. Lights reflect as streaks
/// and as specular glints on the ripples. The glint light direction comes from the light map
/// gradient, so local lights make glints in their own color.
/// Local light also keeps its own color through the water body, so a warm lamp does not turn green.
/// A height shadow also dims the surface a bit. The sky reflection does not change in a real shadow,
/// but without the dim step a pier shadow on the water is almost invisible.
pub const WATER_FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp int;
precision highp sampler2DArray;
in vec4 v_color;
in vec3 v_uv;
out vec4 o_color;
uniform sampler2DArray u_image;
uniform sampler2DArray u_scene_color;
uniform sampler2DArray u_emission;
uniform sampler2DArray u_gbuffer_normal;
uniform sampler2DArray u_light_map;
uniform int u_maps;
uniform vec3 u_transmittance;
uniform vec3 u_scatter;
uniform float u_refraction;
uniform float u_reflectivity;
uniform float u_reach;
uniform float u_roughness;
uniform float u_glints;
uniform float u_shininess;
uniform float u_foam;
uniform float u_caustics;
uniform float u_sparkles;
uniform vec2 u_direction;
uniform vec2 u_ripple_scale;
uniform float u_ripple_strength;
uniform float u_time;
uniform vec2 u_target_size;
uniform vec2 u_scene_size;
uniform vec4 u_view_rect;
uniform float u_texel_world;
uniform vec3 u_sky;
uniform vec3 u_sun;
uniform vec2 u_sun_direction;
uniform vec3 u_ambient;
uniform int u_debug;
uniform sampler2DArray u_shadow;
uniform vec3 u_shadow_light;
uniform vec3 u_shadow_open;
uniform float u_shadow_softness;

SHADOW_MASK_FUNCTION

const int REFLECTION_STEPS = 16;
/// A depth map is zero on the land, so the line where it leaves zero follows whole texels and
/// draws stairs. The water edge sits a bit deeper, where the filtered depth is a smooth line.
const float SHORE_LEVEL = 0.03;
/// The march finds the bank only to one step. A coarse hit puts the mirror line, and the fade,
/// at whole steps, so a curved shore reflects as stairs. A binary search finds the exact edge.
const int REFINE_STEPS = 5;
const int STREAK_TAPS = 8;
const int WATER_VIEW = 9;
const float TAU = 6.283185307179586;

bool submerged(vec2 scene_uv) {
    if (any(lessThan(scene_uv, vec2(0.0))) || any(greaterThan(scene_uv, vec2(1.0)))) {
        return false;
    }
    vec4 normal_sample = texture(u_gbuffer_normal, vec3(scene_uv, 0.0));
    if (normal_sample.a <= 0.0) {
        return false;
    }
    int mode = int(floor(normal_sample.b / normal_sample.a * 4.0 + 0.5));
    return (mode & 1) != 0;
}

vec2 gradient_hash(vec2 cell) {
    vec2 value = vec2(dot(cell, vec2(127.1, 311.7)), dot(cell, vec2(269.5, 183.3)));
    return normalize(fract(sin(value) * 43758.5453) * 2.0 - 1.0 + 1.0e-5);
}

float gradient_noise(vec2 position) {
    vec2 cell = floor(position);
    vec2 local = fract(position);
    vec2 blend = local * local * local * (local * (local * 6.0 - 15.0) + 10.0);
    float a = dot(gradient_hash(cell), local);
    float b = dot(gradient_hash(cell + vec2(1.0, 0.0)), local - vec2(1.0, 0.0));
    float c = dot(gradient_hash(cell + vec2(0.0, 1.0)), local - vec2(0.0, 1.0));
    float d = dot(gradient_hash(cell + vec2(1.0, 1.0)), local - vec2(1.0, 1.0));
    return mix(mix(a, b, blend.x), mix(c, d, blend.x), blend.y);
}

/// A slow swell carries sharp chop. Each chop octave turns and drifts in its own direction,
/// so the pattern never repeats in an obvious way.
float ripple_height(vec2 position) {
    float swell = gradient_noise(position * 0.35 + vec2(0.05, 0.03) * u_time);
    float height = sin(dot(position, vec2(0.6, 0.8)) * 1.3 + swell * 4.0 + u_time * 0.9) * 0.5;
    mat2 turn = mat2(0.8, 0.6, -0.6, 0.8);
    vec2 octave_position = position;
    float amplitude = 0.5;
    for (int octave = 0; octave < 5; ++octave) {
        float speed = 0.35 + 0.2 * float(octave);
        float chop = gradient_noise(octave_position + vec2(speed, speed * 0.6) * u_time);
        height += (0.35 - abs(chop)) * amplitude;
        octave_position = turn * octave_position * 2.03;
        amplitude *= 0.55;
    }
    return height;
}

vec3 ripple_normal(vec2 uv) {
    vec2 position = uv * u_ripple_scale;
    float step_size = 0.01;
    float center = ripple_height(position);
    float dx = ripple_height(position + vec2(step_size, 0.0)) - center;
    float dy = ripple_height(position + vec2(0.0, step_size)) - center;
    vec2 slope = vec2(dx, dy) / step_size * u_ripple_strength * 0.06;
    return normalize(vec3(-slope, 1.0));
}

/// Each cell holds one tiny facet with a small random tilt on top of the ripple normal.
/// The facet flashes only when it mirrors the sun into the camera. A directional sun over a
/// top down view has the same mirror angle everywhere, so the ripple slope must decide it,
/// and the flashes gather on the ripple faces.
float sparkle(vec2 position, vec3 normal, vec3 halfway) {
    vec2 cell = floor(position);
    vec4 random = fract(sin(vec4(
        dot(cell, vec2(12.9898, 78.233)),
        dot(cell, vec2(39.3468, 11.135)),
        dot(cell, vec2(73.156, 52.235)),
        dot(cell, vec2(94.673, 27.791))
    )) * 43758.5453);
    vec2 center = 0.2 + random.xy * 0.6;
    vec3 facet = normalize(normal + vec3((random.zw - 0.5) * 0.12, 0.0));
    float mirror = pow(max(dot(facet, halfway), 0.0), 1500.0);
    float flash = pow(max(sin(u_time * (2.0 + random.x * 5.0) + random.y * TAU), 0.0), 8.0);
    float spot = 1.0 - smoothstep(0.0, 0.2, length(fract(position) - center));
    return spot * flash * mirror;
}

/// Caustics are thin bright lines where the ripples focus the light on the bed.
float caustics(vec2 position) {
    float first = 1.0 - abs(gradient_noise(position * 0.8 + vec2(0.11, 0.07) * u_time));
    float second = 1.0 - abs(gradient_noise(position * 1.3 - vec2(0.05, 0.13) * u_time + 7.3));
    return pow(first, 10.0) + pow(second, 10.0) * 0.7;
}

float luminance(vec3 color) {
    return dot(color, vec3(0.2126, 0.7152, 0.0722));
}

WATER_FUNCTION

void main() {
    if (u_debug != 0 && u_debug != WATER_VIEW) {
        discard;
    }
    vec4 albedo = (u_maps & 4) != 0 ? v_color : texture(u_image, v_uv) * v_color;
    float shoreline = smoothstep(SHORE_LEVEL, SHORE_LEVEL * 2.0, albedo.a);
    if (shoreline <= 0.0) {
        discard;
    }
    vec2 screen_uv = gl_FragCoord.xy / u_target_size;
    vec2 scene_uv = u_view_rect.xy + screen_uv * u_view_rect.zw;
    if (!submerged(scene_uv)) {
        discard;
    }
    float depth = albedo.a;
    vec3 normal = ripple_normal(v_uv.xy);
    if (u_debug == WATER_VIEW) {
        o_color = vec4(vec3(normal.xy * 0.5 + 0.5, depth) * shoreline, shoreline);
        return;
    }
    vec2 world_to_scene = 1.0 / (u_texel_world * u_scene_size);
    vec2 world_to_screen = world_to_scene / u_view_rect.zw;

    vec2 refracted_uv = screen_uv + normal.xy * u_refraction * depth * world_to_screen;
    if (!submerged(u_view_rect.xy + refracted_uv * u_view_rect.zw)) {
        refracted_uv = screen_uv;
    }
    vec3 bed = textureLod(u_scene_color, vec3(refracted_uv, 0.0), depth * 1.5).rgb;
    bed *= 1.0 + u_caustics * caustics(v_uv.xy * u_ripple_scale) * (1.0 - depth * 0.7);
    float shadow = shadow_mask(scene_uv);
    vec3 light_map = textureLod(u_light_map, vec3(scene_uv, 0.0), 1.0).rgb;
    light_map = shadowed(light_map, shadow);
    vec3 light = light_map + u_ambient;
    vec3 sun = u_sun * (1.0 - shadow);
    float local_light = clamp(luminance(max(light_map - sun, vec3(0.0))) / max(luminance(light), 1.0e-4), 0.0, 1.0);
    vec3 transmittance = pow(max(u_transmittance, vec3(1.0e-4)), vec3(depth));
    transmittance = mix(transmittance, vec3(luminance(transmittance)), local_light);
    vec3 body = bed * transmittance + u_scatter * albedo.rgb * light * (vec3(1.0) - transmittance);

    vec2 direction = u_direction + normal.xy * 0.6;
    vec3 reflection = u_sky;
    float step_world = u_reach / float(REFLECTION_STEPS);
    for (int index = 1; index <= REFLECTION_STEPS; ++index) {
        float distance_world = step_world * float(index);
        vec2 probe = scene_uv + direction * distance_world * world_to_scene;
        if (!submerged(probe)) {
            float inside = distance_world - step_world;
            for (int refine = 0; refine < REFINE_STEPS; ++refine) {
                float middle = (inside + distance_world) * 0.5;
                if (submerged(scene_uv + direction * middle * world_to_scene)) {
                    inside = middle;
                } else {
                    distance_world = middle;
                }
            }
            vec2 mirror = screen_uv + direction * distance_world * 2.0 * world_to_screen;
            float fade = 1.0 - distance_world / u_reach;
            vec3 bank = textureLod(u_scene_color, vec3(mirror, 0.0), u_roughness + (1.0 - fade) * 2.0).rgb;
            reflection = mix(u_sky, bank, fade);
            break;
        }
    }

    vec3 streak = vec3(0.0);
    for (int index = 0; index < STREAK_TAPS; ++index) {
        float along = (float(index) + 0.5) / float(STREAK_TAPS);
        vec2 tap = scene_uv + (direction * u_reach * along) * world_to_scene;
        streak += textureLod(u_emission, vec3(tap, 0.0), u_roughness + along * 2.0).rgb * (1.0 - along);
    }
    reflection += streak * u_glints / float(STREAK_TAPS);
    vec3 halo = textureLod(u_emission, vec3(scene_uv, 0.0), 4.0).rgb + textureLod(u_emission, vec3(scene_uv, 0.0), 6.0).rgb * 0.5;

    float facing = clamp(normal.z, 0.0, 1.0);
    float fresnel = u_reflectivity + (1.0 - u_reflectivity) * pow(1.0 - facing, 5.0);

    vec3 halfway_sun = normalize(normalize(vec3(u_sun_direction, 1.0)) + vec3(0.0, 0.0, 1.0));
    vec3 glint = sun * pow(max(dot(normal, halfway_sun), 0.0), u_shininess);
    vec3 halfway_sparkle = normalize(mix(vec3(0.0, 0.0, 1.0), halfway_sun, 0.25));
    vec2 sparkle_position = v_uv.xy * u_ripple_scale * 8.0;
    float sparkles = sparkle(sparkle_position, normal, halfway_sparkle)
        + sparkle(sparkle_position * 1.37 + 17.0, normal, halfway_sparkle);
    glint += sun * sparkles * u_sparkles;
    float texel = 3.0 / u_scene_size.x;
    float lod = 3.0;
    float left = luminance(textureLod(u_light_map, vec3(scene_uv - vec2(texel, 0.0), 0.0), lod).rgb);
    float right = luminance(textureLod(u_light_map, vec3(scene_uv + vec2(texel, 0.0), 0.0), lod).rgb);
    float down = luminance(textureLod(u_light_map, vec3(scene_uv - vec2(0.0, texel), 0.0), lod).rgb);
    float up = luminance(textureLod(u_light_map, vec3(scene_uv + vec2(0.0, texel), 0.0), lod).rgb);
    vec2 gradient = vec2(right - left, up - down) / max(luminance(light), 1.0e-4);
    float local = smoothstep(0.02, 0.3, length(gradient));
    vec3 toward_light = normalize(vec3(gradient / max(length(gradient), 1.0e-4), 0.5));
    vec3 halfway_local = normalize(toward_light + vec3(0.0, 0.0, 1.0));
    glint += light * pow(max(dot(normal, halfway_local), 0.0), u_shininess) * u_glints * local;

    float shore = 1.0 - smoothstep(0.0, 0.15, depth);
    float waves = 0.5 + 0.5 * sin(depth * 70.0 - u_time * 2.0 + ripple_height(v_uv.xy * u_ripple_scale) * 2.0);
    float foam = u_foam * shore * smoothstep(0.35, 0.9, waves * shore + shore * 0.4);
    float sun_share = luminance(u_shadow_light) / max(luminance(u_shadow_open), 1.0e-4);
    o_color = water_color(WaterSurface(
        bed,
        body,
        reflection,
        glint,
        halo * u_glints * 0.6,
        light,
        normal,
        fresnel,
        depth,
        clamp(foam, 0.0, 1.0),
        shadow,
        1.0 - shadow * sun_share * 0.4,
        shoreline,
        v_uv.xy * u_ripple_scale,
        scene_uv,
        screen_uv
    ));
}
"#;
