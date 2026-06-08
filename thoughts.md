1. We need add color and opacity picking for fluid cells bleu and white colors and its border
2. We should consider ripping out marching cubes entirely for now. I really hate how it looks atm, and I'm honestly starting to enjoy the lake of particles look. We could add marching cubes to the future roadmap but it would need to be a ground reimagining of the feature
3. Why does the sim just break on 8milion+ particles. it goes from like 40 fps at 2 million, 30 fps at 4 million, and just not moving at 8 million 
4. How can I improve the performance of the app much further. 
5. Can i fix the wall hugging once and for all. wall friction = 0 does not delete wall friction, and i've tried to get AI to do it many times now. We're missing something crucial.
6. What settings am I missing here? What new settings could add 
7. Making the balls smaller or larger physics wise is very unsatisfying and complicated. there's rest particles/cell, volume stiffness, liquid threshold, surface dilation, draft clamp, pressure iterations. All of the variables seem to affect particle size in various ways. We don't need to remove them, but there does seem to be a lot of overlap here. Can we simplify here? What does the math here do? Ideally we should be able to increase or decrease how compact all the particles are and how compact hte liquid cells are
8. The tooltips are very bad atm. We should probably have two. What does this do tooltip (functionality focused) and technical tooltip (much more complex details about how it works). we should probably greatly shortern and reduce and remove a lot of the simple tooltips, there's way too much text in all these. We should also likely reorder a lot of these
9. A few extra features we could add in: water source, drain, wave makers (various types), auto rotater



