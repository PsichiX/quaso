use spitfire_draw::{
    context::DrawContext,
    utils::{Drawable, Vertex},
};
use spitfire_glow::graphics::GraphicsTarget;
use vek::{Aabr, Rect, Vec2};

#[derive(Debug, Default, Clone)]
pub struct Interactible {
    pub vertices: Vec<Vec2<f32>>,
    pub triangles: Vec<[u32; 3]>,
}

impl Interactible {
    pub fn new(
        vertices: impl IntoIterator<Item = Vec2<f32>>,
        triangles: impl IntoIterator<Item = [u32; 3]>,
    ) -> Self {
        Self {
            vertices: vertices.into_iter().collect(),
            triangles: triangles.into_iter().collect(),
        }
    }

    pub fn from_rect(rect: Rect<f32, f32>) -> Self {
        let vertices = vec![
            Vec2::new(rect.x, rect.y),
            Vec2::new(rect.x + rect.w, rect.y),
            Vec2::new(rect.x + rect.w, rect.y + rect.h),
            Vec2::new(rect.x, rect.y + rect.h),
        ];
        let triangles = vec![[0, 1, 2], [0, 2, 3]];
        Self {
            vertices,
            triangles,
        }
    }

    pub fn vertex(mut self, vertex: impl Into<Vec2<f32>>) -> Self {
        self.add_vertex(vertex);
        self
    }

    pub fn triangle(mut self, triangle: [u32; 3]) -> Self {
        self.add_triangle(triangle);
        self
    }

    pub fn add_vertex(&mut self, vertex: impl Into<Vec2<f32>>) {
        self.vertices.push(vertex.into());
    }

    pub fn add_triangle(&mut self, triangle: [u32; 3]) {
        self.triangles.push(triangle);
    }

    pub fn bounding_box(&self) -> Aabr<f32> {
        if self.vertices.is_empty() {
            return Default::default();
        }

        let mut bbox = Aabr::new_empty(self.vertices[0]);
        for vertex in &self.vertices[1..] {
            bbox = bbox.expanded_to_contain_point(*vertex);
        }
        bbox
    }

    pub fn contains_point(&self, point: impl Into<Vec2<f32>>) -> bool {
        let point = point.into();

        for triangle in &self.triangles {
            let v0 = self.vertices[triangle[0] as usize];
            let v1 = self.vertices[triangle[1] as usize];
            let v2 = self.vertices[triangle[2] as usize];

            let dx = point.x - v2.x;
            let dy = point.y - v2.y;
            let dx21 = v2.x - v1.x;
            let dy12 = v1.y - v2.y;
            let d = dy12 * (v0.x - v2.x) + dx21 * (v0.y - v2.y);
            let s = dy12 * dx + dx21 * dy;
            let t = (v2.y - v0.y) * dx + (v0.x - v2.x) * dy;

            if d < 0.0 {
                if s <= 0.0 && t <= 0.0 && s + t >= d {
                    return true;
                }
            } else if s >= 0.0 && t >= 0.0 && s + t <= d {
                return true;
            }
        }

        false
    }
}

pub struct DrawableInteractible<T: Drawable> {
    pub drawable: T,
    pub interactible: Interactible,
}

impl<T: Drawable> DrawableInteractible<T> {
    pub fn new(drawable: T) -> Self {
        Self {
            drawable,
            interactible: Interactible::default(),
        }
    }

    pub fn draw(
        mut self,
        context: &mut DrawContext,
        graphics: &mut dyn GraphicsTarget<Vertex>,
    ) -> Interactible {
        let vertex_offset = graphics.state().stream.vertices().len();
        let triangle_offset = graphics.state().stream.triangles().len();
        self.drawable.draw(context, graphics);
        self.interactible
            .vertices
            .reserve_exact(graphics.state().stream.vertices().len() - vertex_offset);
        self.interactible
            .triangles
            .reserve_exact(graphics.state().stream.triangles().len() - triangle_offset);
        for vertex in graphics.state().stream.vertices()[vertex_offset..].iter() {
            self.interactible.add_vertex(vertex.position);
        }
        for triangle in graphics.state().stream.triangles()[triangle_offset..].iter() {
            self.interactible.add_triangle([
                triangle.a - vertex_offset as u32,
                triangle.b - vertex_offset as u32,
                triangle.c - vertex_offset as u32,
            ]);
        }
        self.interactible
    }
}
