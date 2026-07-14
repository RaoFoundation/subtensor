import fragmentShader from './shader/fragment.glsl'
import vertexShader from './shader/vertex.glsl'

let requestAnimationFrameID

let oldTime = new Date().getTime()
const FrameRate = 60

export function initCanvas(canvas) {
  //// All of the following code was written by David A. Madore and extended slightly by Unconst.
  //// Full credit goes to David.
  ////   (<URL: http://www.madore.org/~david/ >),
  //// around 2012-01-28, and is hereby placed in the Public Domain.
  ////   (I would appreciate getting some thanks/recognition if it
  ////   is used elsewhere, but this is not a legal requirement,
  ////   just a polite request.)
  //// It comes with no warranty whatsoever, of course.

  // Coordinates of the roots (these are fixed):
  var roots = new Array()

  function createRoots() {
    function rootFirstKind(i0, i1, s0, s1) {
      var rt = [0, 0, 0, 0, 0, 0, 0, 0]
      rt[i0] = s0 ? -2 : 2
      rt[i1] = s1 ? -2 : 2
      return rt
    }

    function rootSecondKind(s0, s1, s2, s3, s4, s5, s6) {
      var s7 = s0 ^ s1 ^ s2 ^ s3 ^ s4 ^ s5 ^ s6
      return [
        s0 ? -1 : 1,
        s1 ? -1 : 1,
        s2 ? -1 : 1,
        s3 ? -1 : 1,
        s4 ? -1 : 1,
        s5 ? -1 : 1,
        s6 ? -1 : 1,
        s7 ? -1 : 1,
      ]
    }

    for (var i0 = 0; i0 < 8; i0++) {
      for (var i1 = i0 + 1; i1 < 8; i1++) {
        roots.push(rootFirstKind(i0, i1, false, false))
        roots.push(rootFirstKind(i0, i1, false, true))
        roots.push(rootFirstKind(i0, i1, true, false))
        roots.push(rootFirstKind(i0, i1, true, true))
      }
    }

    for (var i = 0; i < 128; i++) {
      roots.push(
        rootSecondKind(
          !!(i & 1),
          !!(i & 2),
          !!(i & 4),
          !!(i & 8),
          !!(i & 16),
          !!(i & 32),
          !!(i & 64),
        ),
      )
    }
    roots.sort(function (a, b) {
      // Lexicographic ordering
      for (var k = 0; k < 8; k++) {
        if (a[k] < b[k]) return -1
        else if (a[k] > b[k]) return 1
      }
      return 0
    })
  }

  function sqnorm(a) {
    var ret = 0
    for (var i = 0; i < 8; i++) {
      var d = a[i]
      ret += d * d
    }
    return ret
  }

  function dotprod(a, b) {
    var ret = 0
    for (var i = 0; i < 8; i++) {
      ret += a[i] * b[i]
    }
    return ret
  }

  function sqdist(a, b) {
    var ret = 0
    for (var i = 0; i < 8; i++) {
      var d = a[i] - b[i]
      ret += d * d
    }
    return ret
  }

  // List of edges (an edge connects each even entry of this array
  // to the following):
  var edges
  var colidx
  function createEdges() {
    var edges0 = new Array()
    for (var a = 0; a < roots.length; a++) {
      for (var b = a + 1; b < roots.length; b++) {
        if (sqdist(roots[a], roots[b]) == 8) {
          edges0.push(a)
          edges0.push(b)
        }
      }
    }
    edges = new Uint16Array(edges0)
  }

  var gaussianStore = null
  function gaussian() {
    // Generate a Gaussian variable by Box-Muller.
    if (gaussianStore == null) {
      var u0 = Math.random()
      var u1 = Math.random()
      gaussianStore = Math.sqrt(-2 * Math.log(u0)) * Math.cos(2 * Math.PI * u1)
      return Math.sqrt(-2 * Math.log(u0)) * Math.sin(2 * Math.PI * u1)
    } else {
      var ret = gaussianStore
      gaussianStore = null
      return ret
    }
  }

  // The two 8-vectors determining the projection to a plane,
  // plus the two 8-vectors determining the direction of motion:
  // these should be normed and orthogonal and motion should be orthogonal.
  var projMatrix = new Array()
  var colProj = new Array()
  function gramSchmidt() {
    // Make projMatrix normed and orthogonal.
    var ortho = [[], [0], [0], [0, 1, 2]]
    for (var k = 0; k < ortho.length; k++) {
      var d
      for (var l = 0; l < ortho[k].length; l++) {
        var k2 = ortho[k][l]
        d = dotprod(projMatrix[k], projMatrix[k2])
        for (var i = 0; i < 8; i++) projMatrix[k][i] -= d * projMatrix[k2][i]
      }
      d = Math.sqrt(sqnorm(projMatrix[k]))
      for (var i = 0; i < 8; i++) projMatrix[k][i] /= d
    }
  }

  function chooseProjectionRandom() {
    projMatrix[0] = new Array(8)
    projMatrix[1] = new Array(8)
    projMatrix[2] = new Array(8)
    projMatrix[3] = new Array(8)
    for (var k = 0; k < 8; k++) {
      projMatrix[0][k] = gaussian()
      projMatrix[1][k] = gaussian()
      projMatrix[2][k] = gaussian()
      projMatrix[3][k] = gaussian()
    }
    gramSchmidt()
  }

  function modifyProjection() {
    // Velocity rotation.
    for (var k = 0; k < 8; k++) {
      projMatrix[0][k] += projMatrix[2][k] * 0.001
      projMatrix[1][k] += projMatrix[3][k] * 0.001
      projMatrix[2][k] += gaussian() * 0.001
      projMatrix[3][k] += gaussian() * 0.001
    }
    gramSchmidt()
  }

  var rootProj // Projection of the roots (in canvas coordinates)
  var color // colours (in size 4)
  var edgeProj
  function computeProjections() {
    if (typeof rootProj === 'undefined') {
      rootProj = new Float32Array(roots.length * 2)
      color = new Float32Array(roots.length * 4)
      edgeProj = new Float32Array(edges.length * 8)
    }

    for (var n = 0; n < roots.length; n++) {
      var a = dotprod(projMatrix[0], roots[n]) / 3
      var b = dotprod(projMatrix[1], roots[n]) / 3

      rootProj[2 * n] = a
      rootProj[2 * n + 1] = b
      color[2 * n + 0] = 0
      color[2 * n + 1] = 0.1
      color[2 * n + 2] = 0.6
      color[2 * n + 3] = 0.9
    }

    for (var n = 0; n < edges.length; n += 2) {
      var i = edges[n]
      var j = edges[n + 1]

      edgeProj[n] = rootProj[i]
      edgeProj[n + 1] = rootProj[i + 1]
      edgeProj[n + 2] = rootProj[j]
      edgeProj[n + 3] = rootProj[j + 1]
    }
  }

  var canvas
  var gl
  var shaderProg

  function drawLines(buffers) {
    // Draw the 6720 edges.
    // Set the edges as the indices
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, buffers[0])
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, edges, gl.STATIC_DRAW)

    gl.bindBuffer(gl.ARRAY_BUFFER, buffers[1])
    gl.bufferData(gl.ARRAY_BUFFER, rootProj, gl.STATIC_DRAW)
    gl.vertexAttribPointer(shaderProg.pos, 2, gl.FLOAT, false, 0, 0)
    gl.enableVertexAttribArray(shaderProg.pos)

    gl.bindBuffer(gl.ARRAY_BUFFER, buffers[2])
    gl.bufferData(gl.ARRAY_BUFFER, color, gl.STATIC_DRAW)
    gl.vertexAttribPointer(shaderProg.col, 1, gl.FLOAT, false, 0, 0)
    gl.enableVertexAttribArray(shaderProg.col)
    gl.drawElements(gl.LINES, edges.length, gl.UNSIGNED_SHORT, 0)
  }

  function drawPoints() {
    // Draw points.
  }

  function initGl(canvas, gl) {
    gl.viewport(0, 0, canvas.width, canvas.height)
    gl.clearColor(0, 0, 0, 0)
    gl.enable(gl.SAMPLE_COVERAGE)
    gl.enable(gl.LINE_SMOOTH)
    gl.enable(gl.BLEND)
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA)
    gl.lineWidth(0.1)
    var v = vertexShader
    var f = fragmentShader
    var vs = gl.createShader(gl.VERTEX_SHADER)
    gl.shaderSource(vs, v)
    gl.compileShader(vs)
    var fs = gl.createShader(gl.FRAGMENT_SHADER)
    gl.shaderSource(fs, f)
    gl.compileShader(fs)
    shaderProg = gl.createProgram()
    gl.attachShader(shaderProg, vs)
    gl.attachShader(shaderProg, fs)
    gl.linkProgram(shaderProg)
    gl.useProgram(shaderProg)

    shaderProg.fs = fs
    shaderProg.vs = vs
    shaderProg.col = gl.getAttribLocation(shaderProg, 'col')
    shaderProg.pos = gl.getAttribLocation(shaderProg, 'pos')

    var lineBuf = gl.createBuffer()
    var colourBuf = gl.createBuffer()
    var pointBuf = gl.createBuffer()

    return [lineBuf, pointBuf, colourBuf]
    //gl.bindBuffer(gl.ARRAY_BUFFER, pointBuf);
    //gl.enableVertexAttribArray(shaderProg.pos);
  }

  function computeAndDraw(buffers) {
    gl.clear(gl.COLOR_BUFFER_BIT)
    computeProjections()
    drawLines(buffers)
  }

  function onLoad() {
    if (typeof canvas.getContext != 'function') {
      alert(
        'Your browser does not support the HTML5 <canvas> element.\n' +
          'This page will not function.',
      )
      throw 'canvas unsupported'
    }
    gl = canvas.getContext('webgl', { antialias: true })
    if (gl === null) {
      gl = canvas.getContext('experimental-webgl', { antialias: true })
    }
    if (gl === null) {
      alert(
        'Your browser does not support WebGL.\n' +
          'This page will not function.',
      )
      throw 'webgl unsupported'
    }
    createRoots()
    createEdges()
    // initGlCircle(canvas, gl);
    var buffers = initGl(canvas, gl)
    chooseProjectionRandom()
    var update = function () {

      let newTime = new Date().getTime()

      let updateValue = newTime - oldTime
      if (updateValue > 1000/FrameRate) {
        oldTime = newTime
        computeAndDraw(buffers)
        modifyProjection() 
      }
      requestAnimationFrameID = window.requestAnimationFrame(update)
    }
    update()
  }

  onLoad()
}

export function unmountCanvas() {
  window.cancelAnimationFrame(requestAnimationFrameID)
}
