"""Reference values for omawind's decoder, from eccodes (values, lat/lon)
and pyproj (the grid's rotation from true north), never from omawind."""
import sys, math, eccodes as ec, numpy as np
from pyproj import Proj

path = sys.argv[1]
POINTS = [0, 1, 81, 82, 3000, 4321, 7215]
print(f"# eccodes {ec.codes_get_api_version()}, {path.split('/')[-1]}")
fields = {}
with open(path, "rb") as f:
    while (g := ec.codes_grib_new_from_file(f)) is not None:
        name = ec.codes_get(g, "shortName")
        v = ec.codes_get_values(g)
        lats = ec.codes_get_array(g, "latitudes"); lons = ec.codes_get_array(g, "longitudes")
        fields[name] = v
        print(f"field {name} discipline={ec.codes_get(g,'discipline')} category={ec.codes_get(g,'parameterCategory')} "
              f"number={ec.codes_get(g,'parameterNumber')} level_type={ec.codes_get(g,'typeOfFirstFixedSurface')} "
              f"level={ec.codes_get(g,'level')} nx={ec.codes_get(g,'Nx')} ny={ec.codes_get(g,'Ny')} "
              f"hour={ec.codes_get(g,'forecastTime')} sum={v.sum():.6f} min={v.min():.6f} max={v.max():.6f}")
        for i in POINTS:
            print(f"  value {name} {i} {v[i]:.6f}")
        ec.codes_release(g)
# Grid points' positions, all fields share the grid.
for i in POINTS:
    print(f"latlon {i} {lats[i]:.6f} {lons[i] - 360 if lons[i] > 180 else lons[i]:.6f}")
print(f"latlon_max_index {len(lats)-1}")
# True-north rotation by finite differences in the projection itself.
p = Proj("+proj=lcc +lat_1=38.5 +lat_2=38.5 +lat_0=38.5 +lon_0=-97.5 +R=6371229 +units=m +no_defs")
u, vv = fields["10u"], fields["10v"]
for i in POINTS:
    lat, lon = lats[i], (lons[i] - 360 if lons[i] > 180 else lons[i])
    x0, y0 = p(lon, lat); x1, y1 = p(lon, lat + 1e-4)
    theta = math.degrees(math.atan2(x1 - x0, y1 - y0))  # true north, clockwise from grid +y
    to = math.degrees(math.atan2(u[i], vv[i])) - theta   # true bearing the wind blows toward
    frm = (to + 180) % 360
    speed = math.hypot(u[i], vv[i])
    print(f"wind {i} theta={theta:.6f} speed_ms={speed:.6f} from_deg={frm:.6f}")
