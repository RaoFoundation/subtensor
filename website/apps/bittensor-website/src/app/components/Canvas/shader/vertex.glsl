attribute vec2 col;
attribute vec2 pos;
    
// --> Pass to Fragment Shader
varying vec4 variableC;
void main() {
    
    if (col == vec2(0.1, 0)) {
        variableC = vec4(1, 1, 1, 0.9);
    }
    else {
        variableC = vec4(0, 0, 0, 0.9);
    }
    
    //variableC = vec4(col, 0.8, 0.7);
    gl_Position = vec4(pos, 0.9, 0.9);
}