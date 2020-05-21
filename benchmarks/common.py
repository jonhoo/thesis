def setup(paper_mode):
    import matplotlib
    if paper_mode:
        matplotlib.rc('font', family='serif', size=9)
        matplotlib.rc('text.latex', preamble=['\\usepackage{times,mathptmx}'])
        matplotlib.rc('text', usetex=True)
        matplotlib.rc('figure', figsize=(2.95, 0.6))
        matplotlib.rc('legend', fontsize=8)
        matplotlib.rc('axes', linewidth=0.5)
        matplotlib.rc('lines', linewidth=1)
    else:
        font = {'family' : 'sans-serif',
                'sans-serif' : ['Arial'],
                'size'   : '16'}
        matplotlib.rc('font', **font)
        matplotlib.rc('legend', fontsize=14)
        matplotlib.rc('axes', linewidth=1.0)
        matplotlib.rc('lines', linewidth=2.0, markersize=10.0)
        matplotlib.rc('figure', figsize=(8, 3))
        matplotlib.rcParams['xtick.major.width'] = 1.5
        matplotlib.rcParams['xtick.major.size'] = 10
        matplotlib.rcParams['ytick.major.width'] = 1.5
        matplotlib.rcParams['ytick.major.size'] = 10
        matplotlib.rcParams['ytick.minor.width'] = 1
        matplotlib.rcParams['ytick.minor.size'] = 5

# These are the "Tableau 20" colors as RGB.
# http://tableaufriction.blogspot.ro/2012/11/finally-you-can-use-tableau-data-colors.html
tableau20 = [(31, 119, 180), (174, 199, 232), (255, 127, 14), (255, 187, 120),
             (44, 160, 44), (152, 223, 138), (214, 39, 40), (255, 152, 150),
             (148, 103, 189), (197, 176, 213), (140, 86, 75), (196, 156, 148),
             (227, 119, 194), (247, 182, 210), (127, 127, 127), (199, 199, 199),
             (188, 189, 34), (219, 219, 141), (23, 190, 207), (158, 218, 229)]

# Scale the RGB values to the [0, 1] range, which is the format matplotlib accepts.
for i in range(len(tableau20)):
    r, g, b = tableau20[i]
    tableau20[i] = (r / 255., g / 255., b / 255.)

colors = {
    'read': tableau20[5],
    'write': tableau20[0],
}
