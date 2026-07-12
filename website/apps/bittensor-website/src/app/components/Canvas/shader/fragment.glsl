#ifdef GL_ES
precision highp float;
#endif
// <-- Receive from Vertex Shader
varying vec4 variableC;
            
void main() {
gl_FragColor = variableC;
}